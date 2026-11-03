//! Le tableau de bord de l'entreprise : Eve lit elle-même Constructo AI, l'ERP de l'entreprise, pour la montrer d'un
//! coup d'œil — ses chiffres, ses alertes, ses chantiers en cours, ses factures en retard, sa balance âgée, son
//! pipeline, ses soumissions, les factures de ses fournisseurs, ses achats en route, les heures de ses employés,
//! l'agenda partagé, ses comptes en banque. Elle le fait en cliente MCP, comme Claude Code, mais en lecture seule :
//! elle n'appelle que les outils nommés un à un dans [`OUTILS_LUS`], que la garde tient aussi pour des lectures
//! (`garde::est_lecture_constructo`) — jamais un outil d'écriture, même par erreur.
//!
//! Le planning du tableau de bord — le calendrier, le Gantt et le Kanban (`planning.rs`) — en prend aussi ce qu'il
//! montre de l'entreprise : les chantiers en cours et à venir, les bons de travail, les ventes ouvertes, les factures
//! qui attendent leur solde, les notes du calendrier sur quatre mois, les tableaux du Kanban de l'ERP (leurs colonnes,
//! combien de cartes chacune compte). Le suivi de production — les opérations des bons de travail, les dépendances et
//! les références du Gantt — ne se lit qu'à la demande, quand le Gantt s'ouvre, et pas plus d'une fois par
//! demi-heure ([`SUIVI_INTERVALLE`]) : `suivi_lister_operations` pose treize `ALTER TABLE` sur la table des opérations
//! à chaque appel (vérifié dans `mcp_server.py`, `_ensure_operations_columns_mcp`), trop lourd pour une lecture aux dix
//! minutes. Les demandes de prix n'ont pas d'outil de lecture : le module des appels d'offres de l'ERP n'a pas d'outil
//! MCP.
//!
//! Le transport, celui du serveur (`mcp_server.py`, son bloc `__main__`) : le « Streamable HTTP » de MCP, avec ses
//! sessions et ses réponses en flux SSE. Un POST JSON-RPC à `<serveur>/mcp` — `initialize`, la notification
//! `notifications/initialized`, puis un `tools/call` par outil —, l'en-tête `Mcp-Session-Id` que donne le serveur à
//! l'ouverture, renvoyé à chaque requête, et un DELETE qui ferme la session. Une réponse vient en `application/json` ou
//! en `text/event-stream` : les deux se lisent ([`message_de_la_reponse`]).
//!
//! Le jeton : celui d'accès que le chef donne à chaque lecture — jamais un renouvellement d'ici. Le jeton de
//! renouvellement change à chaque renouvellement, et le serveur révoque toute la famille si un ancien resservait : seul
//! le chef renouvelle (`connexion.rs`). Un jeton refusé se dit au chef ([`Panne::Jeton`]), qui en décide.
//!
//! Ce qu'Eve en tire, `entreprise.json` du dossier local ([`Entreprise`] : sa forme est le contrat avec le tableau de
//! bord), se garde d'un démarrage à l'autre : le tableau le montre tout de suite, avec son âge. Seul le chef l'écrit,
//! d'un geste — la lecture qui commence (`en_cours`), qui réussit (`a_jour`), qui manque (`erreur` : la lecture d'avant
//! reste, avec son heure), la connexion absente (`non_connecte` : rien de l'entreprise). Au journal, une ligne par
//! lecture : sa durée et ce qu'elle a trouvé, en nombres ; jamais une donnée de l'entreprise, jamais un jeton.
//!
//! Les formes que rend chaque outil, du texte ou du JSON, sont lues ici d'après `mcp_server.py` ; les essais en gardent
//! un exemple de chacune, et le faux serveur d'`eve --essai-constructo` les rend telles quelles.
//!
//! La comptabilité de l'ERP (le lot 13.2 : l'onglet « Comptabilité », [`crate::compta`]) se lit à part, avec le même
//! client et le même jeton, et seulement à la demande de son onglet qui se montre ([`lire_la_compta`]) : le registre
//! des factures, clients et fournisseurs, par pages de mille (`comptabilite_lister_registre`, jusqu'à
//! `compta::FACTURES_MAX`), puis l'historique de la comptabilité, par pages de cinq cents
//! (`comptabilite_lister_historique`, jusqu'à `compta::CHANGEMENTS_MAX`). Le chef en décide le moment
//! ([`DemandesDeLaCompta`]) et l'écrit dans `registre.json` du dossier local ([`FichierDuRegistre`]). Un serveur
//! d'avant ces deux outils le dit en une phrase ([`COMPTA_PAS_ENCORE`]), jamais en panne obscure.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eve::garde::{ecourter, est_lecture_constructo};
use eve::parole::normaliser;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::centre::{date_du_numero, jours_entre, milliers, numero_du_jour};
use crate::compta;
use crate::connexion::sans_porteur;
use crate::https;
use crate::reglages::ecrire_d_un_geste;

/// La dernière lecture, dans le dossier local.
pub const FICHIER: &str = "entreprise.json";
/// Une lecture toutes les dix minutes, le serveur répondant.
pub const INTERVALLE: Duration = Duration::from_secs(10 * 60);
/// Après une lecture manquée, la suivante attend ceci, puis le double, jusqu'à l'intervalle.
pub const APRES_UNE_PANNE: Duration = Duration::from_secs(60);
/// L'ouverture de la session : Render réveille un service endormi en une trentaine de secondes, parfois plus.
const DELAI_REVEIL: Duration = Duration::from_secs(90);
/// Un appel d'outil, le serveur éveillé.
const DELAI_APPEL: Duration = Duration::from_secs(45);
/// La version de MCP que demande Eve ; le serveur répond la sienne, qu'elle reprend.
const VERSION_MCP: &str = "2025-06-18";
/// Le nom sous lequel Eve se présente au serveur (il le garde dans ses journaux) : rien de la personne.
const NOM_CLIENT: &str = "Eve de Constructo AI (tableau de bord)";
/// Ce que garde le fichier : des listes courtes, celles des cartes du tableau.
const PROJETS_MAX: usize = 20;
const FACTURES_MAX: usize = 25;
const DEVIS_MAX: usize = 10;
const SOUMISSIONS_MAX: usize = 15;
const FACTURES_RECUES_MAX: usize = 15;
/// Les achats en route : tant de chaque sorte (à commander, commandés, livraisons).
const COMMANDES_PAR_SORTE: usize = 7;
const EMPLOYES_MAX: usize = 20;
const PAR_PROJET_MAX: usize = 10;
/// Les notes du calendrier de l'ERP, sur la fenêtre du planning (`planning::fenetre`) : autant que l'outil en rend
/// d'office. La carte de l'agenda du tableau de bord n'en montre que celles qui viennent.
const NOTES_MAX: usize = 200;
/// Les bons de travail (les plus récents, de tout statut), les ventes ouvertes, les factures ouvertes.
const BONS_TRAVAIL_MAX: usize = 50;
const OPPORTUNITES_MAX: usize = 30;
const FACTURES_OUVERTES_MAX: usize = 40;
/// Le suivi de production : autant d'opérations que l'outil en rend au plus.
const OPERATIONS_MAX: usize = 500;
/// Le suivi de production se relit au plus une fois par demi-heure, à la demande (le Gantt qui s'ouvre).
pub const SUIVI_INTERVALLE: Duration = Duration::from_secs(30 * 60);
/// La conformité à renouveler (licences RBQ, cartes CCQ, attestations) : dans ces jours-là ; sous ce seuil, l'alerte
/// est haute.
const CONFORMITE_JOURS: i64 = 60;
const CONFORMITE_URGENTE: i64 = 14;
/// Un nom, une note : pas plus sur le tableau.
const TEXTE_MAX: usize = 200;

/// Les états du fichier.
pub const A_JOUR: &str = "a_jour";
pub const EN_COURS: &str = "en_cours";
pub const NON_CONNECTE: &str = "non_connecte";
pub const ERREUR: &str = "erreur";

/// Les seuls outils de Constructo AI qu'Eve appelle elle-même : des lectures, nommées une à une (leurs formes :
/// `mcp_server.py`). Un outil s'ajoute ici, jamais ailleurs ; [`permis`] demande en plus que la garde le tienne pour
/// une lecture. Les derniers, ceux du suivi de production ([`OUTILS_DU_SUIVI`]) et ceux de la comptabilité
/// ([`OUTILS_DE_LA_COMPTA`]), seulement à la demande.
pub const OUTILS_LUS: [&str; 24] = [
    "tableau_kpis",
    "tableau_alertes",
    "tableau_factures_agees",
    "lister_projets",
    "tableau_pipeline_ventes",
    "lister_devis",
    "lister_pointages",
    "suivi_lister_notes_calendrier",
    "lister_factures",
    "lister_bons_commande",
    "lister_bons_achat",
    "lister_livraisons",
    "banque_lister_comptes",
    "conformite_expirations",
    // Le planning : les bons de travail, les ventes, les tableaux du Kanban (leurs colonnes, leurs comptes).
    "lister_bons_travail",
    "lister_opportunites",
    "suivi_colonnes",
    "suivi_etat_kanban",
    // Le suivi de production, à la demande.
    "suivi_lister_operations",
    "suivi_lister_dependances_gantt",
    "suivi_lister_baselines",
    "lister_employes",
    // La comptabilité, à la demande de son onglet.
    compta::OUTIL_DU_REGISTRE,
    compta::OUTIL_DE_L_HISTORIQUE,
];

/// Le suivi de production : les opérations des bons de travail (leur responsable nommé d'après la liste des employés),
/// les dépendances et les références du Gantt. Ils ne se lisent qu'à la demande ([`SUIVI_INTERVALLE`]).
pub const OUTILS_DU_SUIVI: [&str; 4] =
    ["suivi_lister_operations", "suivi_lister_dependances_gantt", "suivi_lister_baselines", "lister_employes"];

/// La comptabilité de l'ERP : le registre des factures et l'historique, lus à part de la lecture de l'entreprise,
/// seulement quand l'onglet « Comptabilité » se montre ([`lire_la_compta`]).
pub const OUTILS_DE_LA_COMPTA: [&str; 2] = [compta::OUTIL_DU_REGISTRE, compta::OUTIL_DE_L_HISTORIQUE];

/// Un outil qu'Eve peut appeler elle-même : nommé dans [`OUTILS_LUS`] et tenu pour une lecture par la garde — les
/// deux, pour qu'une erreur d'un côté ne laisse jamais passer une écriture.
pub fn permis(outil: &str) -> bool {
    OUTILS_LUS.contains(&outil) && est_lecture_constructo(outil)
}

/// Les appels d'une lecture, de l'essentiel au reste : un serveur qui flanche en route a déjà rendu les chiffres.
/// Les pointages, en deux appels (cette semaine, la précédente) : l'outil en rend 200 au plus ; les projets aussi (en
/// cours, puis à venir). `suivi` : le suivi de production en plus, à la fin ([`OUTILS_DU_SUIVI`]).
fn appels(aujourdhui: &str, suivi: bool) -> Vec<(&'static str, Value)> {
    let jour = |decalage: i64| numero_du_jour(aujourdhui).map(|n| date_du_numero(n + decalage)).unwrap_or_default();
    // Le lundi de cette semaine, en jours depuis aujourd'hui (le 1er janvier 1970, jour zéro, était un jeudi).
    let lundi = numero_du_jour(aujourdhui).map_or(0, |n| -(n + 3).rem_euclid(7));
    let (debut, fin) = crate::planning::fenetre(aujourdhui);
    let mut a = vec![
        ("tableau_kpis", json!({ "periode_jours": 30 })),
        ("tableau_alertes", json!({})),
        ("tableau_factures_agees", json!({ "inclure_detail": true, "limite": FACTURES_MAX })),
        ("lister_projets", json!({ "statut": "En cours", "limite": 50 })),
        ("tableau_pipeline_ventes", json!({})),
        // Les 50 devis les plus récents : les soumissions en cours, et ceux qui attendent une réponse.
        ("lister_devis", json!({ "limite": 50 })),
        ("lister_pointages", json!({ "date_debut": jour(lundi), "date_fin": jour(lundi + 6), "limite": 200 })),
        ("lister_pointages", json!({ "date_debut": jour(lundi - 7), "date_fin": jour(lundi - 1), "limite": 200 })),
        // Les notes du calendrier sur la fenêtre du planning : de quoi tourner ses pages d'un mois à l'autre.
        ("suivi_lister_notes_calendrier", json!({ "date_debut": debut, "date_fin": fin, "limite": NOTES_MAX })),
        // Les factures de l'ERP mêlent clients et fournisseurs : celles des fournisseurs se trient ici.
        ("lister_factures", json!({ "limite": 100 })),
        ("lister_bons_commande", json!({ "limite": 30 })),
        ("lister_bons_achat", json!({ "limite": 30 })),
        ("lister_livraisons", json!({ "statut": "planifiee", "limite": 30 })),
        ("banque_lister_comptes", json!({})),
        ("conformite_expirations", json!({ "jours": CONFORMITE_JOURS })),
        // Le planning : les chantiers à venir, les bons de travail, les ventes, le Kanban de l'ERP.
        ("lister_projets", json!({ "statut": "En attente", "limite": 50 })),
        ("lister_bons_travail", json!({ "limite": BONS_TRAVAIL_MAX })),
        ("lister_opportunites", json!({ "limite": 50 })),
        ("suivi_colonnes", json!({})),
        ("suivi_etat_kanban", json!({})),
    ];
    if suivi {
        // Autant que chaque outil en rend : 500 opérations, 1000 dépendances et références, 200 employés.
        a.extend(OUTILS_DU_SUIVI.iter().map(|&o| {
            let limite = match o {
                "suivi_lister_operations" => OPERATIONS_MAX,
                "lister_employes" => 200,
                _ => 1000,
            };
            (o, json!({ "limite": limite }))
        }));
    }
    a
}

/// Les outils d'une lecture, chacun une fois.
fn outils_de(appels: &[(&'static str, Value)]) -> Vec<&'static str> {
    let mut outils: Vec<&'static str> = Vec::new();
    for (o, _) in appels {
        if !outils.contains(o) {
            outils.push(o);
        }
    }
    outils
}

// ---- Le fichier : entreprise.json --------------------------------------------------------------------------------

/// `entreprise.json` : la dernière lecture de l'ERP, et son état. Chaque champ est facultatif ; un champ absent, Eve ne
/// l'a pas (sa forme, pour le tableau de bord : `entreprise.md` de l'équipe).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entreprise {
    /// L'heure de la dernière lecture réussie (ISO 8601, le décalage du poste).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lu_a: Option<String>,
    /// `a_jour`, `en_cours`, `non_connecte` ou `erreur`.
    pub etat: String,
    /// L'heure de la dernière tentative, réussie ou non.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub essai_a: Option<String>,
    /// Pourquoi la dernière lecture a manqué, en une phrase pour l'écran.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub erreur: Option<String>,
    /// `false` : Constructo AI n'est pas permis à cette Eve.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permis: Option<bool>,
    /// Les outils qui n'ont rien rendu de lisible à cette lecture : leurs champs sont absents.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub manques: Vec<String>,
    #[serde(skip_serializing_if = "Kpis::vides")]
    pub kpis: Kpis,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alertes: Option<Vec<Alerte>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projets: Option<Vec<Projet>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factures_en_retard: Option<Vec<FactureEnRetard>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance_agee: Option<BalanceAgee>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<Vec<Etape>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agenda: Option<Vec<Note>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devis: Option<Vec<Devis>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banque: Option<Vec<Compte>>,
    /// Les soumissions en cours (brouillon, validée, envoyée, en attente), la dernière modifiée d'abord.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soumissions: Option<Vec<Devis>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factures_recues: Option<Vec<FactureRecue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commandes: Option<Vec<Commande>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heures: Option<Heures>,
    // ---- Le planning (le calendrier, le Gantt, le Kanban) ----
    /// Les chantiers à venir (« En attente » dans l'ERP).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projets_en_attente: Option<Vec<Projet>>,
    /// Les bons de travail les plus récents, de tout statut.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bons_travail: Option<Vec<BonDeTravail>>,
    /// Les ventes ouvertes du CRM (ni gagnées ni perdues).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opportunites: Option<Vec<Opportunite>>,
    /// Les factures qui attendent leur solde, des clients et des fournisseurs, la plus proche échéance d'abord.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factures_ouvertes: Option<Vec<FactureOuverte>>,
    /// Les tableaux du Kanban de l'ERP : leurs colonnes, leur règle, leurs comptes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kanban: Option<Vec<TableauDuKanban>>,
    /// Le suivi de production, lu à la demande : l'heure de sa dernière lecture (ISO 8601), ses opérations, les
    /// dépendances et les références du Gantt. Une lecture sans lui garde ceux d'avant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suivi_lu_a: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operations: Option<Vec<Operation>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependances: Option<Vec<DependanceDuGantt>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<Reference>>,
}

/// Les chiffres d'un coup d'œil.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Kpis {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projets_en_cours: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valeur_projets_en_cours: Option<f64>,
    /// Le solde dû des comptes clients.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a_recevoir: Option<f64>,
    /// Le solde des factures échues, et leur nombre.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub en_retard: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factures_en_retard: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devis_en_attente: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valeur_devis_en_attente: Option<f64>,
    /// La valeur des opportunités ouvertes (ni gagnées ni perdues).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pipeline: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opportunites_ouvertes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encaissements_30_jours: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub produits_sous_seuil: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clients_actifs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub employes_actifs: Option<u64>,
    /// Les devis acceptés, en pour cent de tous.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taux_conversion_devis: Option<f64>,
    /// La somme des soldes du dernier relevé importé, des comptes qui en ont un.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solde_banque: Option<f64>,
    /// Les achats en route : les bons d'achat à commander, les commandes attendues, les livraisons prévues.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bons_a_commander: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commandes_ouvertes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub livraisons_prevues: Option<u64>,
}

impl Kpis {
    fn vides(&self) -> bool {
        *self == Kpis::default()
    }
}

/// Ce qui demande d'agir : son texte (pour l'écran et pour la voix), sa gravité (`haute`, `moyenne`, `info`), sa sorte
/// (`factures`, `stock`, `conformite`, `banque`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Alerte {
    pub texte: String,
    pub gravite: String,
    pub sorte: String,
}

/// Un projet en cours (ou à venir) ; ses dates (AAAA-MM-JJ) sont celles de son début et de sa fin dans l'ERP. Aucun
/// outil ne rend l'avancement d'une liste de projets : l'ERP le calcule de leurs phases. `id` : son numéro dans l'ERP,
/// celui que donnent ses bons de travail et les références du Gantt.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Projet {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub nom: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numero: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valeur: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ville: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fin: Option<String>,
    /// L'adresse du chantier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adresse: Option<String>,
}

/// Une facture échue, son solde, ses jours de retard.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FactureEnRetard {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numero: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    pub solde: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jours: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub echeance: Option<String>,
}

/// Le solde dû, par ancienneté de l'échéance.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BalanceAgee {
    pub a_venir: f64,
    #[serde(rename = "0_30")]
    pub de_0_a_30: f64,
    #[serde(rename = "31_60")]
    pub de_31_a_60: f64,
    #[serde(rename = "61_90")]
    pub de_61_a_90: f64,
    pub plus_de_90: f64,
    pub total: f64,
}

/// Une étape ouverte du pipeline.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Etape {
    pub etape: String,
    pub nombre: u64,
    pub valeur: f64,
}

/// Une note partagée du calendrier de l'ERP : son jour (sans heure), son texte.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    pub quand: String,
    pub texte: String,
}

/// Un devis (une soumission au client) : `depuis`, le jour où il a été soumis (sinon créé) ; `modifie`, le jour de sa
/// dernière modification ; `debut` et `fin`, ceux où ses travaux sont prévus (`date_prevu`, `date_fin` : le Gantt des
/// devis de l'ERP les prend). L'ERP ne lui garde pas d'adresse : elle est d'ordinaire dans son nom. `id` : son numéro
/// dans l'ERP.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Devis {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numero: Option<String>,
    pub nom: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    pub statut: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valeur: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depuis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifie: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fin: Option<String>,
}

/// Une facture reçue d'un fournisseur : son numéro dans l'ERP, le fournisseur, le projet (s'il est en cours), sa date,
/// son montant, son solde, son statut.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FactureRecue {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numero: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fournisseur: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub montant: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solde: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statut: Option<String>,
}

/// Un achat en route : `sorte` — `a_commander` (un bon d'achat pas encore commandé ; `date` : son échéance), `commande`
/// (un bon de commande pas encore reçu ; `depuis` : le jour où il a été commandé ; `date` : la livraison prévue),
/// `livraison` (une livraison de chantier prévue ; `date` et `heure`). `texte` : ses notes, courtes (d'une livraison :
/// sa sorte et sa zone). `id` : son numéro dans l'ERP.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Commande {
    pub sorte: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numero: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fournisseur: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depuis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub montant: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texte: Option<String>,
}

/// Les heures des employés, d'après leurs pointages : cette semaine (du lundi `semaine` au dimanche) et la précédente.
/// `a_venir` : des heures saisies pour des jours après aujourd'hui (à vérifier) ; `manquantes` : le dernier jour de
/// semaine avant aujourd'hui, et ceux qui n'y ont aucune heure (parmi ceux qui en ont dans les deux semaines).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Heures {
    pub semaine: String,
    pub cette_semaine: f64,
    pub semaine_passee: f64,
    pub a_venir: f64,
    pub employes_actifs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derniere_saisie: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manquantes: Option<Manquantes>,
    pub employes: Vec<HeuresEmploye>,
    /// Les heures de cette semaine, par projet.
    pub par_projet: Vec<HeuresProjet>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Manquantes {
    pub jour: String,
    pub employes: Vec<String>,
}

/// Les heures d'un employé : ses deux semaines, ce qu'il a saisi d'avance, s'il est au travail (un pointage ouvert
/// aujourd'hui), et ses jours (des deux semaines, ceux qui ont des heures).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeuresEmploye {
    pub nom: String,
    pub cette_semaine: f64,
    pub semaine_passee: f64,
    pub a_venir: f64,
    pub au_travail: bool,
    pub jours: Vec<HeuresJour>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeuresJour {
    pub jour: String,
    pub heures: f64,
}

/// Les heures de la semaine sur un projet : son nom (« Autre projet » s'il n'est pas en cours, « Sans projet »).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HeuresProjet {
    pub nom: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numero: Option<String>,
    pub heures: f64,
}

/// Un compte bancaire actif : le solde du dernier relevé importé (s'il y en a un), celui des opérations publiées, et
/// les opérations à classer.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Compte {
    pub nom: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solde: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solde_publie: Option<f64>,
    pub a_classer: u64,
}

/// Un bon de travail (`lister_bons_travail`) : son numéro dans l'ERP (`id`) et son numéro de document, son nom, son
/// statut, sa priorité, son chantier (son numéro dans l'ERP, et son nom s'il est en cours ou à venir), son client, son
/// montant, ses dates de début et de fin, son échéance.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BonDeTravail {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numero: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nom: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priorite: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projet_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub montant: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub echeance: Option<String>,
}

/// Une vente ouverte du CRM (`lister_opportunites`) : son numéro dans l'ERP, son nom, son étape (`statut`), son montant
/// estimé, sa probabilité (en pour cent), son début (le début prévu, sinon le jour où elle a été créée : le Gantt des
/// ventes de l'ERP le prend) et sa clôture prévue, d'où elle vient (`origine`, la « source » du CRM).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Opportunite {
    pub id: u64,
    pub nom: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub montant: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probabilite: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origine: Option<String>,
}

/// Une facture qui attend son solde (`lister_factures`, ni brouillon ni annulée) : `sorte` — `client` (à recevoir) ou
/// `fournisseur` (à payer) —, son numéro dans l'ERP et le sien, le client ou le fournisseur (`tiers`), le projet (s'il
/// est en cours ou à venir), son statut, son jour (`date`) et son échéance, son solde.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FactureOuverte {
    pub sorte: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numero: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiers: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub echeance: Option<String>,
    pub solde: f64,
}

/// Un tableau du Kanban de l'ERP (`projets`, `devis`, `bons_travail`, `achats`, `factures`, `operations`,
/// `operations_sdp`, `ventes`) : ses colonnes dans l'ordre de l'ERP (`suivi_colonnes` ; vide quand l'ERP n'en donne
/// pas, comme pour les ventes), sa règle (`garde`), combien de cartes il compte en tout et par colonne
/// (`suivi_etat_kanban`, le statut tel que la base l'écrit).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TableauDuKanban {
    pub cle: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub statuts: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub garde: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub par_colonne: BTreeMap<String, u64>,
}

/// Une opération du suivi de production (`suivi_lister_operations`) : son numéro dans l'ERP, son bon de travail (son
/// numéro dans l'ERP et son numéro de document), son nom, son statut, ses dates, ses heures prévues et faites (son
/// avancement, comme l'ERP le calcule), qui la fait (`responsable`, nommé d'après la liste des employés ; jamais
/// autre chose de lui), son fournisseur (« Interne », ou le sous-traitant).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Operation {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bt_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bt_numero: Option<String>,
    pub nom: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debut: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heures_prevues: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heures_reelles: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsable: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fournisseur: Option<String>,
}

/// Une dépendance du Gantt de l'ERP (`suivi_lister_dependances_gantt`) : la tâche `vers` attend la tâche `de` — chacune
/// une sorte (`project`, `bt`, `op`, `devis`, `bc`, `opp`) et son numéro dans l'ERP —, de la façon que dit `sorte`
/// (`finish_to_start`…), de tant de jours (`decalage`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DependanceDuGantt {
    pub de_sorte: String,
    pub de_id: String,
    pub vers_sorte: String,
    pub vers_id: String,
    pub sorte: String,
    pub decalage: i64,
}

/// La référence d'une tâche du Gantt (sa ligne de base, `suivi_lister_baselines`) : sa sorte et son numéro dans l'ERP,
/// et les dates qui avaient été prévues.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Reference {
    pub sorte: String,
    pub id: String,
    pub debut: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fin: Option<String>,
}

impl Entreprise {
    /// Eve n'est pas reliée à Constructo AI (ou il ne lui est pas permis) : rien de l'entreprise — une déconnexion
    /// efface ce qui restait —, seulement pourquoi.
    pub fn non_connecte(permis: bool) -> Entreprise {
        Entreprise {
            etat: NON_CONNECTE.into(),
            permis: (!permis).then_some(false),
            erreur: (!permis)
                .then(|| "Constructo AI n'est pas permis à cette Eve (reglages.json, « privileges »).".to_string()),
            ..Entreprise::default()
        }
    }

    /// Une lecture commence : la précédente reste, avec son heure.
    pub fn en_cours(&self) -> Entreprise {
        Entreprise { etat: EN_COURS.into(), erreur: None, permis: None, ..self.clone() }
    }

    /// Une lecture réussie, faite à `a` (ISO 8601).
    pub fn lue(lecture: &Lecture, a: &str) -> Entreprise {
        Entreprise {
            lu_a: Some(a.into()),
            etat: A_JOUR.into(),
            essai_a: Some(a.into()),
            manques: lecture.manques.clone(),
            ..lecture.entreprise.clone()
        }
    }

    /// Une lecture manquée à `a` : la dernière réussie reste, avec son heure ; `erreur` dit pourquoi.
    pub fn manquee(&self, erreur: &str, a: &str) -> Entreprise {
        Entreprise {
            etat: ERREUR.into(),
            erreur: Some(erreur.into()),
            essai_a: Some(a.into()),
            permis: None,
            ..self.clone()
        }
    }

    /// La lecture qui suit celle-ci, réussie à `a` : la nouvelle ([`Entreprise::lue`]), avec le suivi de production de
    /// celle-ci quand la nouvelle ne l'a pas relu (il ne se lit qu'à la demande) — ou, de lui, ce qu'un outil qui a
    /// flanché n'a pas rendu. L'heure du suivi est celle de sa dernière lecture, réussie ou non : la demi-heure qu'il
    /// attend avant la suivante part de là.
    pub fn suite(&self, lecture: &Lecture, a: &str) -> Entreprise {
        let mut e = Entreprise::lue(lecture, a);
        e.suivi_lu_a = if lecture.suivi { Some(a.into()) } else { self.suivi_lu_a.clone() };
        if e.operations.is_none() {
            e.operations = self.operations.clone();
        }
        if e.dependances.is_none() {
            e.dependances = self.dependances.clone();
        }
        if e.references.is_none() {
            e.references = self.references.clone();
        }
        e
    }
}

/// Le suivi de production est-il à relire ? Jamais lu, ou lu il y a plus d'une demi-heure ([`SUIVI_INTERVALLE`]) — une
/// heure illisible, ou dans l'avenir (l'horloge du poste a reculé), aussi.
pub fn suivi_a_relire(e: &Entreprise) -> bool {
    let intervalle = SUIVI_INTERVALLE.as_secs() as i64;
    e.suivi_lu_a.as_deref().and_then(secondes_depuis).is_none_or(|s| !(0..intervalle).contains(&s))
}

/// La dernière lecture gardée dans le dossier local ; rien (ou illisible) : une lecture vide.
pub fn charger(dossier: &Path) -> Entreprise {
    fs::read(dossier.join(FICHIER)).ok().and_then(|o| serde_json::from_slice(&o).ok()).unwrap_or_default()
}

/// Écrit la lecture dans le dossier local, d'un geste.
pub fn ecrire(dossier: &Path, e: &Entreprise) -> Result<(), String> {
    let texte = serde_json::to_vec_pretty(e).map_err(|err| err.to_string())?;
    ecrire_d_un_geste(&dossier.join(FICHIER), &texte).map_err(|err| format!("{FICHIER} ne s'écrit pas ({err})"))
}

// ---- Une lecture ---------------------------------------------------------------------------------------------------

/// Pourquoi une lecture n'a rien donné, ou un appel non plus.
#[derive(Clone, Debug, PartialEq)]
pub enum Panne {
    /// Le serveur refuse le jeton d'accès (401) : le chef en décide (un jeton neuf).
    Jeton,
    /// Le serveur ne répond pas, ou pas comme il faut (le réseau, Render qui se réveille trop longtemps, une panne) :
    /// la lecture s'arrête là ; une autre, plus tard.
    Serveur(String),
    /// Le serveur a oublié la session (404 : il a redémarré entre deux appels) : elle se rouvre, une fois.
    Session,
    /// L'outil n'a rien rendu de bon (son erreur) : la lecture continue sans lui.
    Outil(String),
    /// Un outil hors de la liste d'Eve : jamais envoyé.
    Refuse(String),
    /// Le serveur ne connaît pas cet outil (« Unknown tool » : un serveur d'avant lui) : il manque à la lecture de
    /// l'entreprise, comme un outil qui flanche ; la comptabilité le dit en une phrase ([`COMPTA_PAS_ENCORE`]).
    Inconnu(String),
    /// Le serveur refuse l'accès lui-même (403) : un jeton neuf n'y changerait rien ; la lecture s'arrête, et la
    /// comptabilité ne garde rien de l'entreprise ([`refus_d_autorisation`]).
    Interdit,
}

impl Panne {
    /// En une phrase, pour l'écran et le journal (jamais un jeton : le serveur ne les cite pas, et ce qu'il dit passe
    /// par `sans_porteur`).
    pub fn texte(&self) -> String {
        match self {
            Panne::Jeton => "Constructo AI refuse le jeton d'Eve".into(),
            Panne::Serveur(m) | Panne::Outil(m) => m.clone(),
            Panne::Session => "Constructo AI a oublié la session".into(),
            Panne::Refuse(o) => format!("l'outil {o} n'est pas une lecture permise à Eve"),
            Panne::Inconnu(o) => format!("{o} : un outil que Constructo AI ne connaît pas"),
            Panne::Interdit => raison_http(403),
        }
    }
}

/// Le serveur dit-il qu'il ne connaît pas l'outil ? FastMCP le rend en résultat d'erreur (`isError`, « Unknown tool:
/// comptabilite_lister_registre » : `mcp/server/fastmcp/tools/tool_manager.py`), la spécification de MCP en erreur
/// JSON-RPC (le code -32602, « Unknown tool: … ») ; d'autres serveurs disent « Tool not found ».
fn outil_inconnu(message: &str) -> bool {
    let m = message.to_lowercase();
    m.contains("unknown tool") || m.contains("tool not found")
}

/// Une lecture réussie : ce qu'elle a tiré de l'ERP (les données seulement : le chef y met l'état et l'heure), les
/// outils qui n'ont rien rendu de lisible, ses temps, combien d'outils elle a appelés, et si elle a lu le suivi de
/// production.
#[derive(Clone, Debug)]
pub struct Lecture {
    pub entreprise: Entreprise,
    pub manques: Vec<String>,
    /// L'ouverture de la session (le réveil du serveur, s'il dormait), et toute la lecture.
    pub ouverture: Duration,
    pub duree: Duration,
    pub outils: usize,
    pub suivi: bool,
}

impl Lecture {
    /// Ce que la lecture a trouvé, en nombres, pour le journal : jamais une donnée de l'entreprise.
    pub fn resume(&self) -> String {
        let e = &self.entreprise;
        fn n<T>(l: &Option<Vec<T>>) -> usize {
            l.as_ref().map_or(0, Vec::len)
        }
        let sans = if self.manques.is_empty() { String::new() } else { format!(" ; sans {}", self.manques.join(", ")) };
        let employes = e.heures.as_ref().map_or(0, |h| h.employes.len());
        let suivi = if self.suivi {
            format!(
                " ; le suivi de production : {} opérations, {} dépendances, {} références",
                n(&e.operations),
                n(&e.dependances),
                n(&e.references)
            )
        } else {
            String::new()
        };
        format!(
            "{} outils lus sur {} en {} (la session ouverte en {}) : {} projets en cours, {} à venir, {} factures en \
             retard, {} alertes, {} notes d'agenda, {} soumissions, {} devis en attente, {} factures reçues, {} \
             factures ouvertes, {} achats en route, {employes} employés pointés, {} comptes bancaires, {} bons de \
             travail, {} ventes ouvertes, {} tableaux du Kanban{suivi}{sans}",
            self.outils.saturating_sub(self.manques.len()),
            self.outils,
            secondes(self.duree),
            secondes(self.ouverture),
            n(&e.projets),
            n(&e.projets_en_attente),
            n(&e.factures_en_retard),
            n(&e.alertes),
            n(&e.agenda),
            n(&e.soumissions),
            n(&e.devis),
            n(&e.factures_recues),
            n(&e.factures_ouvertes),
            n(&e.commandes),
            n(&e.banque),
            n(&e.bons_travail),
            n(&e.opportunites),
            n(&e.kanban),
        )
    }
}

/// « 3,2 s ».
pub fn secondes(d: Duration) -> String {
    format!("{:.1} s", d.as_secs_f64()).replace('.', ",")
}

/// Une lecture dans un fil (elle parle au serveur, qui dort parfois) : `rapporter` reçoit son temps et ce qu'elle a
/// donné ; une panique y devient une panne. `suivi` : le suivi de production en plus ([`OUTILS_DU_SUIVI`]).
pub fn lire_dans_un_fil(
    ressource: String,
    jeton: String,
    aujourdhui: String,
    suivi: bool,
    rapporter: impl FnOnce(Duration, Result<Lecture, Panne>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let debut = Instant::now();
        let lecture = || lire(&ressource, &jeton, &aujourdhui, suivi);
        let issue = std::panic::catch_unwind(std::panic::AssertUnwindSafe(lecture))
            .unwrap_or_else(|_| Err(Panne::Serveur("la lecture de Constructo AI a planté (voir eve.log)".into())));
        rapporter(debut.elapsed(), issue);
    });
}

/// Une lecture de l'ERP : une session MCP au connecteur (`ressource` : `<serveur>/mcp`), un appel par outil, puis la
/// session fermée. Un outil qui flanche manque à la lecture ; un serveur qui flanche l'arrête ; aucun outil lisible :
/// une panne. `suivi` : le suivi de production aussi, après tout le reste.
pub fn lire(ressource: &str, jeton: &str, aujourdhui: &str, suivi: bool) -> Result<Lecture, Panne> {
    let debut = Instant::now();
    let mut client = Client::ouvrir(ressource, jeton, None)?;
    let ouverture = debut.elapsed();
    let appels = appels(aujourdhui, suivi);
    let outils = outils_de(&appels).len();
    let mut reponses: Vec<(&str, Result<String, String>)> = Vec::new();
    for (outil, arguments) in appels {
        match client.appeler(outil, arguments) {
            Ok(texte) => reponses.push((outil, Ok(texte))),
            Err(p @ (Panne::Outil(_) | Panne::Refuse(_) | Panne::Inconnu(_))) => reponses.push((outil, Err(p.texte()))),
            Err(p) => {
                client.fermer();
                return Err(p);
            }
        }
    }
    client.fermer();
    let (entreprise, manques) = analyser(&reponses, aujourdhui);
    if manques.len() == outils {
        let raison = reponses.iter().find_map(|(_, r)| r.as_ref().err()).cloned().unwrap_or_default();
        return Err(Panne::Serveur(format!("Constructo AI n'a rien rendu de lisible ({})", ecourter(&raison, 160))));
    }
    Ok(Lecture { entreprise, manques, ouverture, duree: debut.elapsed(), outils, suivi })
}

// ---- La comptabilité : registre.json --------------------------------------------------------------------------------

/// Ce que dit Eve quand Constructo AI n'a pas encore les deux outils de la comptabilité (un serveur d'avant eux : ils
/// arrivent avec une mise à jour de l'ERP, qu'une Eve peut précéder) : une lecture manquée comme une autre, dite en une
/// phrase — jamais une panne obscure, ni des relectures en boucle (la suivante attend la demande d'après).
pub const COMPTA_PAS_ENCORE: &str =
    "Constructo AI n'offre pas encore la lecture de la comptabilité : elle viendra avec sa prochaine mise à jour.";

/// La comptabilité se relit à la demande de son onglet, qui la redemande toutes les deux minutes tant qu'il se montre :
/// une demande ordinaire, au plus une fois par dix minutes ; « Relire » (ou « Réessayer »), au plus une fois par trente
/// secondes. Comptées depuis le début de la lecture d'avant, à cinq secondes près : la cinquième demande de l'onglet
/// tombe à dix minutes, à une poussière près — sans cette marge, chaque lecture glisserait à douze.
pub const COMPTA_INTERVALLE: Duration = Duration::from_secs(10 * 60);
pub const COMPTA_RELIRE: Duration = Duration::from_secs(30);
const COMPTA_MARGE: Duration = Duration::from_secs(5);
/// Un serveur sans les outils de la comptabilité ([`FinDeLecture::Absente`]) : une demande ordinaire par heure, pas
/// une par dix minutes — sa mise à jour les apportera, et la séance doit la voir sans « Relire » (la QA5, passe 2 :
/// l'onglet disait « n'offre pas encore » toute la séance après la mise à jour de l'ERP).
pub const COMPTA_ABSENTE: Duration = Duration::from_secs(60 * 60);
/// Une lecture de la comptabilité entière ne dure jamais plus : un outil bloqué chez le serveur — son flux SSE envoie «
/// : ping » toutes les 15 s, ce qui relançait sans fin chaque attente de WinHTTP (la QA5 du lot 13.2) — ne laisse plus
/// « Lecture en cours… » à l'écran pour toujours. Chaque requête n'en prend que ce qui reste ; au-delà, la lecture
/// manque et le dit ([`LECTURE_TROP_LONGUE`]).
pub const COMPTA_DUREE_MAX: Duration = Duration::from_secs(5 * 60);
pub const LECTURE_TROP_LONGUE: &str =
    "Constructo AI a mis plus de cinq minutes à rendre la comptabilité : Eve a arrêté la lecture";

/// Combien de factures et d'entrées de l'historique Eve demande par appel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pages {
    pub registre: usize,
    pub historique: usize,
}

/// Les pages d'une lecture : les bornes du serveur (1000 factures, 500 entrées).
pub const PAGES: Pages = Pages { registre: 1000, historique: 500 };
/// Jamais plus de pages : un serveur qui rendrait des pages de quelques factures ne fait pas durer une lecture des
/// heures (deux cents pages de cent factures font déjà `compta::FACTURES_MAX`). Au-delà, la liste se dit tronquée.
const PAGES_DU_REGISTRE_MAX: usize = 200;
const PAGES_DE_L_HISTORIQUE_MAX: usize = 20;

/// Une lecture de la comptabilité réussie : le registre (les factures, l'historique ; le chef y met l'état et l'heure)
/// et, pour le journal, en nombres, ce qu'elle a été : ses pages, ce qui ne s'y lisait pas, ses temps.
#[derive(Clone, Debug, Default)]
pub struct LectureDeLaCompta {
    pub registre: compta::Registre,
    pub pages: usize,
    pub pages_de_l_historique: usize,
    /// Des factures ou des entrées laissées de côté (sans id, ou en double) ; des pages qui ne se lisaient pas (leur
    /// liste s'arrête là, et se dit tronquée).
    pub lignes_ecartees: usize,
    pub pages_illisibles: usize,
    /// Le jeton d'accès refusé (un 401) pendant l'historique : le registre lu reste, et le chef fait renouveler le
    /// jeton comme après une lecture refusée.
    pub jeton_refuse: bool,
    /// L'ouverture de la session (le réveil du serveur, s'il dormait), et toute la lecture.
    pub ouverture: Duration,
    pub duree: Duration,
}

impl LectureDeLaCompta {
    /// Ce que la lecture a trouvé, en nombres, pour le journal : jamais une donnée de l'entreprise.
    pub fn resume(&self) -> String {
        let r = &self.registre;
        let n = |x: usize, un: &str, des: &str| format!("{x} {}", if x > 1 { des } else { un });
        let mut s = format!(
            "{} en {} ({} au serveur), {} en {}, lue en {} (la session ouverte en {})",
            n(r.factures.len(), "facture", "factures"),
            n(self.pages, "page", "pages"),
            r.au_serveur,
            n(r.historique.len(), "entrée d'historique", "entrées d'historique"),
            n(self.pages_de_l_historique, "page", "pages"),
            secondes(self.duree),
            secondes(self.ouverture),
        );
        for (vrai, quoi) in [
            (r.tronque, "le registre tronqué"),
            (r.historique_tronque, "l'historique tronqué"),
            (r.historique_absent, "pas de journal d'audit"),
        ] {
            if vrai {
                s.push_str(&format!(" ; {quoi}"));
            }
        }
        for (x, un, des) in [
            (r.colonnes_absentes.len(), "colonne absente", "colonnes absentes"),
            (self.lignes_ecartees, "ligne écartée", "lignes écartées"),
            (self.pages_illisibles, "page illisible", "pages illisibles"),
        ] {
            if x > 0 {
                s.push_str(&format!(" ; {}", n(x, un, des)));
            }
        }
        s
    }
}

/// Une lecture de la comptabilité dans un fil (elle parle au serveur, qui dort parfois) : `rapporter` reçoit son temps
/// et ce qu'elle a donné ; une panique y devient une panne.
pub fn lire_la_compta_dans_un_fil(
    ressource: String,
    jeton: String,
    rapporter: impl FnOnce(Duration, Result<LectureDeLaCompta, Panne>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let debut = Instant::now();
        let lecture = || lire_la_compta(&ressource, &jeton, PAGES, COMPTA_DUREE_MAX);
        let issue = std::panic::catch_unwind(std::panic::AssertUnwindSafe(lecture))
            .unwrap_or_else(|_| Err(Panne::Serveur("la lecture de la comptabilité a planté (voir eve.log)".into())));
        rapporter(debut.elapsed(), issue);
    });
}

/// Une lecture de la comptabilité : une session MCP au connecteur (`ressource` : `<serveur>/mcp`), le registre puis
/// l'historique, page par page ([`lire_les_pages`]), puis la session fermée. `pages` : leur taille ([`PAGES`] ; un
/// essai les fait petites, pour en lire plusieurs) ; `duree` : la lecture entière n'en prend jamais plus
/// ([`COMPTA_DUREE_MAX`] ; chaque requête n'a que ce qui en reste).
pub fn lire_la_compta(ressource: &str, jeton: &str, pages: Pages, duree: Duration) -> Result<LectureDeLaCompta, Panne> {
    let debut = Instant::now();
    let echeance = debut + duree;
    let mut client = Client::ouvrir(ressource, jeton, Some(echeance))?;
    let ouverture = debut.elapsed();
    let lu =
        lire_les_pages(&mut |outil: &str, arguments: Value| client.appeler(outil, arguments), pages, Some(echeance));
    client.fermer();
    let mut l = lu?;
    l.ouverture = ouverture;
    l.duree = debut.elapsed();
    Ok(l)
}

/// Les deux listes, page par page ; `appeler` rend le texte d'un outil (le client MCP ; un essai le remplace). Le
/// registre d'abord, du plus récent au plus ancien (`recentes_d_abord`, `avant_id`), jusqu'à sa dernière page ou les
/// `compta::FACTURES_MAX` plus récentes : à la borne, ce sont les plus anciennes qui manquent, jamais celles du moment
/// (une lecture du plus ancien au plus récent gardait les vingt mille premières, et les indicateurs du mois étaient
/// faux : la QA5 du lot 13.2). Puis l'historique, du plus récent au plus ancien (`avant_id`), jusqu'à sa dernière page
/// ou `compta::CHANGEMENTS_MAX` entrées. Une panne du serveur pendant le registre — son texte d'erreur, un outil qu'il
/// ne connaît pas, le réseau, le jeton, l'échéance passée — fait manquer la lecture (le chef garde alors la dernière) ;
/// pendant l'historique, elle l'arrête là sans jeter le registre lu, et se dit (`historique_erreur`). Une facture ou
/// une entrée qui ne se lit pas est écartée et comptée ; une page qui ne se lit pas arrête sa liste, dite tronquée, et
/// se compte — sauf la première du registre : sans rien de lisible, la lecture manque. `echeance` : au-delà, plus rien
/// ne se demande.
fn lire_les_pages(
    appeler: &mut dyn FnMut(&str, Value) -> Result<String, Panne>,
    pages: Pages,
    echeance: Option<Instant>,
) -> Result<LectureDeLaCompta, Panne> {
    let passee = || echeance.is_some_and(|e| Instant::now() >= e);
    let mut l = LectureDeLaCompta::default();
    // Le registre, du plus récent au plus ancien.
    let mut factures: Vec<compta::Facture> = Vec::new();
    let (mut lues, mut total, mut tronque) = (0usize, 0u64, false);
    let mut colonnes: Vec<String> = Vec::new();
    let mut avant = 0i64;
    loop {
        if passee() {
            return Err(Panne::Serveur(LECTURE_TROP_LONGUE.into()));
        }
        let arguments = json!({ "recentes_d_abord": true, "avant_id": avant, "limite": pages.registre });
        let texte = appeler(compta::OUTIL_DU_REGISTRE, arguments)?;
        let page = match page_lue(&texte, "factures") {
            Ok(p) => p,
            Err(PageManquee::Erreur(e)) => return Err(Panne::Outil(e)),
            Err(PageManquee::Illisible) if l.pages == 0 => {
                return Err(Panne::Serveur("Constructo AI a rendu un registre illisible".into()));
            }
            Err(PageManquee::Illisible) => {
                l.pages_illisibles += 1;
                tronque = true;
                break;
            }
        };
        l.pages += 1;
        total = total.max(page.total.unwrap_or(0));
        for c in page.colonnes_absentes {
            if !colonnes.contains(&c) {
                colonnes.push(c);
            }
        }
        lues += page.lignes.len();
        for v in &page.lignes {
            match compta::Facture::du_serveur(v) {
                Some(f) => factures.push(f),
                None => l.lignes_ecartees += 1,
            }
        }
        if factures.len() >= compta::FACTURES_MAX {
            tronque = factures.len() > compta::FACTURES_MAX || page.suivant != Suivant::Fin;
            // Les plus récentes (le plus grand id), même d'une page qui ne viendrait pas dans l'ordre.
            factures.sort_unstable_by(|a, b| b.id.cmp(&a.id));
            factures.truncate(compta::FACTURES_MAX);
            break;
        }
        match page.suivant {
            Suivant::Fin => break,
            // Un `suivant` plus petit que la page d'avant : la liste descend.
            Suivant::Id(s) if s > 0 && (avant == 0 || s < avant) => {
                if l.pages >= PAGES_DU_REGISTRE_MAX {
                    tronque = true;
                    break;
                }
                avant = s;
            }
            // Un `suivant` qui ne se lit pas, ou qui ne mène pas plus loin : la liste s'arrête là.
            _ => {
                l.pages_illisibles += 1;
                tronque = true;
                break;
            }
        }
    }
    // Dans l'ordre des id (le contrat), chacune une fois.
    factures.sort_by_key(|f| f.id);
    let lues_une_fois = factures.len();
    factures.dedup_by_key(|f| f.id);
    l.lignes_ecartees += lues_une_fois - factures.len();
    // L'historique, dans l'ordre du serveur : le plus récent d'abord. Sa panne ne jette pas le registre lu (elle le
    // faisait : la QA5 du lot 13.2) : il s'arrête là, tronqué, et elle se dit.
    let mut historique: Vec<compta::Changement> = Vec::new();
    let mut vus: HashSet<i64> = HashSet::new();
    let (mut absent, mut historique_tronque) = (false, false);
    let mut historique_erreur = String::new();
    let mut avant_id = 0i64;
    loop {
        if passee() {
            historique_tronque = true;
            historique_erreur = phrase_de_l_historique(&Panne::Serveur(LECTURE_TROP_LONGUE.into()));
            break;
        }
        let arguments = json!({ "avant_id": avant_id, "limite": pages.historique });
        let texte = match appeler(compta::OUTIL_DE_L_HISTORIQUE, arguments) {
            Ok(t) => t,
            // Un 403 refuse l'accès lui-même, pas l'historique seul : la lecture entière manque, et rien ne reste.
            Err(p @ Panne::Interdit) => return Err(p),
            Err(p) => {
                l.jeton_refuse = p == Panne::Jeton;
                historique_tronque = true;
                historique_erreur = phrase_de_l_historique(&p);
                break;
            }
        };
        let page = match page_lue(&texte, "changements") {
            Ok(p) => p,
            Err(PageManquee::Erreur(e)) => {
                historique_tronque = true;
                historique_erreur = phrase_de_l_historique(&Panne::Outil(e));
                break;
            }
            Err(PageManquee::Illisible) => {
                l.pages_illisibles += 1;
                historique_tronque = true;
                break;
            }
        };
        l.pages_de_l_historique += 1;
        if page.table_absente {
            absent = true;
            break;
        }
        for v in &page.lignes {
            match compta::Changement::du_serveur(v) {
                Some(c) if vus.insert(c.id) => historique.push(c),
                _ => l.lignes_ecartees += 1,
            }
        }
        if historique.len() >= compta::CHANGEMENTS_MAX {
            historique_tronque = historique.len() > compta::CHANGEMENTS_MAX || page.suivant != Suivant::Fin;
            historique.truncate(compta::CHANGEMENTS_MAX);
            break;
        }
        match page.suivant {
            Suivant::Fin => break,
            Suivant::Id(s) if s > 0 && (avant_id == 0 || s < avant_id) => {
                if l.pages_de_l_historique >= PAGES_DE_L_HISTORIQUE_MAX {
                    historique_tronque = true;
                    break;
                }
                avant_id = s;
            }
            _ => {
                l.pages_illisibles += 1;
                historique_tronque = true;
                break;
            }
        }
    }
    l.registre = compta::Registre {
        au_serveur: total.max(lues as u64),
        factures,
        tronque,
        colonnes_absentes: colonnes,
        historique,
        historique_absent: absent,
        historique_tronque,
        historique_erreur,
    };
    Ok(l)
}

/// Où mène une page : nulle part (la dernière), à l'id à redonner au serveur, ou on ne sait où (ce qu'il a écrit ne se
/// lit pas).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Suivant {
    Fin,
    Id(i64),
    Illisible,
}

/// Une page de l'un des deux outils : ses lignes (`factures` ou `changements`), où elle mène, et ce que le serveur en
/// dit — combien de factures il a, les colonnes que sa base n'a pas, un journal d'audit qui n'existe pas encore.
#[derive(Debug)]
struct PageLue {
    lignes: Vec<Value>,
    suivant: Suivant,
    total: Option<u64>,
    colonnes_absentes: Vec<String>,
    table_absente: bool,
}

/// Pourquoi une page ne se lit pas : le serveur a rendu son texte d'erreur au lieu du JSON (« Erreur lors de… » : une
/// panne de lecture, dite telle quelle), ou un JSON qui n'est pas celui du contrat.
#[derive(Debug, PartialEq)]
enum PageManquee {
    Erreur(String),
    Illisible,
}

/// Une page telle que la rend l'outil : un TEXTE JSON compact, un objet (le contrat : `mcp_server.py`) ; `cle`, la
/// liste qu'il porte. Un texte qui n'est pas un objet JSON est l'erreur du serveur.
fn page_lue(texte: &str, cle: &str) -> Result<PageLue, PageManquee> {
    let t = texte.trim();
    if t.is_empty() {
        return Err(PageManquee::Illisible);
    }
    if !t.starts_with('{') {
        return Err(PageManquee::Erreur(ecourter(&sans_porteur(t), 200)));
    }
    let Ok(Value::Object(mut o)) = serde_json::from_str::<Value>(t) else { return Err(PageManquee::Illisible) };
    let table_absente = o.get("table_absente").and_then(Value::as_bool) == Some(true);
    let lignes = match o.remove(cle) {
        Some(Value::Array(l)) => l,
        None | Some(Value::Null) if table_absente => Vec::new(),
        _ => return Err(PageManquee::Illisible),
    };
    let suivant = match o.get("suivant") {
        None | Some(Value::Null) => Suivant::Fin,
        Some(v) => v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            .map_or(Suivant::Illisible, Suivant::Id),
    };
    let total = o.get("total").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())));
    let colonnes_absentes = o
        .get("colonnes_absentes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|c| ecourter(c, 64))
        .collect();
    Ok(PageLue { lignes, suivant, total, colonnes_absentes, table_absente })
}

/// Pourquoi une lecture de la comptabilité a manqué, dit pour la personne (l'onglet le montre) : le serveur qui n'a pas
/// encore ses outils ([`COMPTA_PAS_ENCORE`]), le jeton refusé, la lecture qu'un administrateur n'a pas ouverte à
/// l'entreprise (le refus de `mcp_profils`), l'erreur du serveur entre guillemets, ou la panne telle qu'elle se dit.
pub fn phrase_de_la_compta(p: &Panne) -> String {
    match p {
        Panne::Inconnu(_) => COMPTA_PAS_ENCORE.to_string(),
        Panne::Jeton => "Constructo AI a refusé le jeton d'Eve : elle relira la comptabilité avec le suivant.".into(),
        Panne::Outil(m) => {
            let m = sans_le_nom_de_l_outil(m);
            if refus_de_profil(m) {
                "La lecture de la comptabilité n'est pas activée pour ton entreprise dans Constructo AI : un \
                 administrateur peut l'ajouter depuis Configuration \u{2192} MCP."
                    .into()
            } else if refus_de_tenant(m) {
                REFUS_DE_TENANT.into()
            } else if base_occupee(m) {
                "La base de Constructo AI était occupée : rien de la comptabilité n'a été lu. Réessaie dans un moment."
                    .into()
            } else {
                format!("Constructo AI n'a pas pu lire la comptabilité : « {} ».", m.trim_end_matches(['.', ' ']))
            }
        }
        autre => ponctuee(autre.texte()),
    }
}

/// Le refus d'une connexion qui ne porte aucune entreprise, dit par Eve.
const REFUS_DE_TENANT: &str = "Constructo AI ne rattache la connexion d'Eve à aucune entreprise : reconnecte-la avec \
                               les identifiants de ton entreprise.";

/// Le refus du serveur quand la connexion ne porte aucune entreprise (`_REG_REFUS_TENANT` de `mcp_server.py` : «
/// Refus : aucune entreprise n'est rattachee a cette connexion… ») : un refus d'accès, comme celui de `mcp_profils`
/// (la QA5, passe 2 : il gardait le registre d'avant).
fn refus_de_tenant(m: &str) -> bool {
    m.starts_with("Refus :") || m.starts_with("Refus:")
}

/// Une lecture que la base a coupée — un verrou en file, son délai dépassé (`_reg_message_erreur` : « Erreur : la base
/// est occupee ou la lecture … a pris trop de temps ») : dite par Eve, pas en texte brut sans accents (la QA5, passe 2).
fn base_occupee(m: &str) -> bool {
    m.starts_with("Erreur : la base est occupee")
}

/// Pourquoi l'historique s'est arrêté avant sa fin, dit pour la personne (`Registre::historique_erreur`) : le registre
/// lu, lui, reste.
pub fn phrase_de_l_historique(p: &Panne) -> String {
    match p {
        Panne::Inconnu(_) => "Constructo AI n'offre pas encore l'historique de la comptabilité : il viendra avec sa \
                              prochaine mise à jour."
            .into(),
        Panne::Jeton => "Constructo AI a refusé le jeton d'Eve pendant la lecture de l'historique : il se relira avec \
                         le suivant."
            .into(),
        Panne::Serveur(t) if t == LECTURE_TROP_LONGUE => {
            "Constructo AI a mis plus de cinq minutes à rendre la comptabilité : l'historique s'est arrêté là.".into()
        }
        Panne::Outil(m) => {
            let m = sans_le_nom_de_l_outil(m);
            if refus_de_profil(m) {
                "La lecture de l'historique n'est pas activée pour ton entreprise dans Constructo AI : un \
                 administrateur peut l'ajouter depuis Configuration \u{2192} MCP."
                    .into()
            } else if refus_de_tenant(m) {
                REFUS_DE_TENANT.into()
            } else if base_occupee(m) {
                "La base de Constructo AI était occupée pendant la lecture de l'historique : il s'est arrêté là.".into()
            } else {
                format!("Constructo AI n'a pas pu lire l'historique : « {} ».", m.trim_end_matches(['.', ' ']))
            }
        }
        autre => format!("L'historique s'est arrêté avant sa fin — {}", ponctuee(autre.texte())),
    }
}

/// La panne refuse-t-elle l'accès lui-même — pas une lecture qui flanche, une lecture qui n'est pas permise ? Un 403
/// ([`Panne::Interdit`]), un outil qu'Eve ne s'accorde pas ([`Panne::Refuse`]), le refus de `mcp_profils` (« n'est pas
/// activé pour votre entreprise »), celui d'une connexion sans entreprise (« Refus : »). Alors rien de l'entreprise ne
/// reste — ni à l'écran ni dans `registre.json`, que lit Claude (la QA5 du lot 13.2) : une lecture d'avant, permise
/// alors, ne l'est plus.
pub fn refus_d_autorisation(p: &Panne) -> bool {
    match p {
        Panne::Interdit | Panne::Refuse(_) => true,
        Panne::Outil(m) => {
            let m = sans_le_nom_de_l_outil(m);
            refus_de_profil(m) || refus_de_tenant(m)
        }
        _ => false,
    }
}

/// Le texte d'un outil en panne, sans le nom de l'outil que le client met devant (« comptabilite_lister_registre : …
/// »).
fn sans_le_nom_de_l_outil(m: &str) -> &str {
    OUTILS_DE_LA_COMPTA.iter().find_map(|o| m.strip_prefix(format!("{o} : ").as_str())).unwrap_or(m).trim()
}

/// Le refus de `mcp_profils.cabler` : « L'outil « … » n'est pas activé pour votre entreprise. Un administrateur peut
/// l'ajouter depuis Configuration → MCP. »
fn refus_de_profil(m: &str) -> bool {
    m.contains("n'est pas activé")
}

/// Une phrase avec son point final.
fn ponctuee(t: String) -> String {
    if t.ends_with(['.', '!', '?']) { t } else { format!("{t}.") }
}

/// Ce que dit `registre.json` du dossier local ([`compta::FICHIER`] ; la forme de [`compta::Lecture`]), tel que le chef
/// le tient : l'état de la lecture, l'heure de la dernière réussie (celle du poste, « AAAA-MM-JJTHH:MM:SS »), pourquoi
/// la dernière a manqué, et le registre — partagé, pour que la lecture qui commence (`en_cours`) ou qui manque
/// (`erreur`) le réécrive tel quel, sans le recopier (vingt mille factures pèsent).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FichierDuRegistre {
    pub etat: String,
    pub lu_le: String,
    pub erreur: String,
    pub registre: Option<Arc<compta::Registre>>,
}

/// `registre.json` tel qu'il s'écrit : la forme de `compta::Lecture`, son registre emprunté.
#[derive(Serialize)]
struct RegistreEcrit<'a> {
    etat: &'a str,
    lu_le: &'a str,
    erreur: &'a str,
    registre: Option<&'a compta::Registre>,
}

impl FichierDuRegistre {
    /// Eve n'est pas reliée à Constructo AI : rien de la comptabilité — une déconnexion efface ce qui restait.
    pub fn non_connecte() -> FichierDuRegistre {
        FichierDuRegistre { etat: NON_CONNECTE.into(), ..FichierDuRegistre::default() }
    }

    /// Une lecture commence : la précédente reste, avec son heure.
    pub fn en_cours(&self) -> FichierDuRegistre {
        FichierDuRegistre { etat: EN_COURS.into(), erreur: String::new(), ..self.clone() }
    }

    /// Une lecture réussie, faite à `lu_le` (l'heure du poste, « AAAA-MM-JJTHH:MM:SS »).
    pub fn lu(registre: compta::Registre, lu_le: &str) -> FichierDuRegistre {
        FichierDuRegistre {
            etat: A_JOUR.into(),
            lu_le: lu_le.into(),
            erreur: String::new(),
            registre: Some(Arc::new(registre)),
        }
    }

    /// Une lecture manquée : la dernière réussie reste, avec son heure ; `erreur` dit pourquoi, pour la personne.
    pub fn manque(&self, erreur: &str) -> FichierDuRegistre {
        FichierDuRegistre { etat: ERREUR.into(), erreur: erreur.into(), ..self.clone() }
    }

    /// Une lecture que Constructo AI refuse (un 403, la lecture qu'un administrateur n'a pas ouverte à l'entreprise :
    /// [`refus_d_autorisation`]) : rien de l'entreprise ne reste — ni la lecture d'avant, permise alors, ni son heure ;
    /// `erreur` dit pourquoi.
    pub fn refusee(erreur: &str) -> FichierDuRegistre {
        FichierDuRegistre { etat: ERREUR.into(), erreur: erreur.into(), ..FichierDuRegistre::default() }
    }

    /// À la première demande ordinaire de la séance : la comptabilité est-elle à relire ? Lue il y a dix minutes ou
    /// plus ([`COMPTA_INTERVALLE`], qui survit ainsi à un redémarrage), jamais lue, son heure illisible ou dans
    /// l'avenir (l'horloge du poste a reculé), ou une lecture restée « en cours » (Eve s'est arrêtée pendant).
    /// `maintenant` : l'heure du poste, « AAAA-MM-JJ HH:MM:SS ».
    pub fn a_relire(&self, maintenant: &str) -> bool {
        if self.etat == EN_COURS || self.registre.is_none() {
            return true;
        }
        let intervalle = COMPTA_INTERVALLE.saturating_sub(COMPTA_MARGE).as_secs() as i64;
        match (crate::rappels::en_secondes(&self.lu_le), crate::rappels::en_secondes(maintenant)) {
            (Some(lu), Some(m)) => !(0..intervalle).contains(&(m - lu)),
            _ => true,
        }
    }

    /// Écrit `registre.json` dans le dossier local, d'un geste : du JSON compact (vingt mille factures font une
    /// dizaine de mégaoctets ; indentées, la moitié de plus), la forme de `compta::Lecture`.
    pub fn ecrire(&self, dossier: &Path) -> Result<(), String> {
        let e = RegistreEcrit {
            etat: &self.etat,
            lu_le: &self.lu_le,
            erreur: &self.erreur,
            registre: self.registre.as_deref(),
        };
        let texte = serde_json::to_vec(&e).map_err(|err| err.to_string())?;
        ecrire_d_un_geste(&dossier.join(compta::FICHIER), &texte)
            .map_err(|err| format!("{} ne s'écrit pas ({err})", compta::FICHIER))
    }

    /// La dernière lecture gardée dans le dossier local ; rien, ou illisible : une lecture vide (jamais lue).
    pub fn charger(dossier: &Path) -> FichierDuRegistre {
        match crate::centre::lire_le_registre(&dossier.join(compta::FICHIER)) {
            Ok(l) => {
                FichierDuRegistre { etat: l.etat, lu_le: l.lu_le, erreur: l.erreur, registre: l.registre.map(Arc::new) }
            }
            Err(_) => FichierDuRegistre::default(),
        }
    }

    /// `registre.json` n'a rien à effacer : il n'existe pas, ou il ne dit déjà que `non_connecte` (sans lire en entier
    /// un gros fichier : c'est une lecture de l'entreprise, à effacer).
    pub fn rien_a_effacer(dossier: &Path) -> bool {
        let chemin = dossier.join(compta::FICHIER);
        match fs::metadata(&chemin) {
            Err(_) => true,
            Ok(m) if m.len() > 4096 => false,
            Ok(_) => {
                crate::centre::lire_le_registre(&chemin).is_ok_and(|l| l.etat == NON_CONNECTE && l.registre.is_none())
            }
        }
    }
}

/// L'écriture de `registre.json`, dans un fil à part : vingt mille factures s'écrivent en un dixième de seconde et plus
/// (mesuré : `mesure_d_un_grand_registre`), deux fois par lecture, que le fil du chef ne passe pas à attendre. Les
/// états s'écrivent dans l'ordre où le chef les donne — d'un geste chacun ([`FichierDuRegistre::ecrire`]) — ; s'il en
/// attend plusieurs pendant une écriture, seul le dernier s'écrit : c'est lui que le fichier doit dire. Le chef parti,
/// le fil finit ce qu'il a et s'arrête.
pub struct EcrivainDuRegistre {
    envoi: std::sync::mpsc::Sender<FichierDuRegistre>,
}

impl EcrivainDuRegistre {
    /// Le fil qui écrit dans `dossier` (le dossier local).
    pub fn demarrer(dossier: std::path::PathBuf) -> EcrivainDuRegistre {
        let (envoi, recu) = std::sync::mpsc::channel::<FichierDuRegistre>();
        std::thread::spawn(move || {
            while let Ok(mut f) = recu.recv() {
                while let Ok(plus_recent) = recu.try_recv() {
                    f = plus_recent;
                }
                if let Err(e) = f.ecrire(&dossier) {
                    crate::journal::ecrire(&format!("comptabilité : {e}"));
                }
            }
        });
        EcrivainDuRegistre { envoi }
    }

    /// `registre.json` dira `f` (un partage du registre : rien ne se recopie).
    pub fn ecrire(&self, f: FichierDuRegistre) {
        let _ = self.envoi.send(f);
    }
}

/// Les demandes de lecture de la comptabilité, telles que le chef les mène (pur : l'heure arrive en argument) : la
/// demande qui attend son moment (pas avant cet instant ; et si c'est « Relire »), la lecture qui se fait (une à la
/// fois), le début de la dernière de la séance, le jeton qu'une lecture a vu refusé (son émission), un serveur qui n'a
/// pas les outils de la comptabilité. [`LaCompta`] y met les gestes de l'onglet et les battements du chef.
#[derive(Clone, Debug, Default)]
pub struct DemandesDeLaCompta {
    voulue: Option<(Instant, bool)>,
    occupee: bool,
    partie_a: Option<Instant>,
    refuse: Option<u64>,
    absente: bool,
}

/// Comment une lecture de la comptabilité a fini, pour ses demandes ([`DemandesDeLaCompta::finie`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinDeLecture {
    /// Lue : un serveur qui n'avait pas les outils les a maintenant.
    Lue,
    /// Manquée d'une panne : la suivante attend son moment.
    Manquee,
    /// Le jeton de cette émission refusé (un 401) : la demande revient, et attend le jeton suivant.
    JetonRefuse(u64),
    /// Le serveur n'a pas les outils de la comptabilité ([`Panne::Inconnu`]) : une demande ordinaire par heure
    /// seulement ([`COMPTA_ABSENTE`] : rien ne les fera venir d'ici dix minutes, ils arrivent avec une mise à jour de
    /// l'ERP) ; « Relire » essaie encore.
    Absente,
}

impl DemandesDeLaCompta {
    /// L'onglet la demande (`relire` : « Relire », « Réessayer ») : une demande ordinaire, si la dernière lecture de la
    /// séance a commencé il y a dix minutes au moins — la première de la séance, si `a_relire` (ce qu'en dit
    /// `registre.json` : [`FichierDuRegistre::a_relire`]) ; « Relire », dès que trente secondes ont passé depuis le
    /// début de la dernière — plus tôt, elle attend la fin des trente secondes. Pendant une lecture, rien : celle qui
    /// se fait y répond. Un serveur sans les outils de la comptabilité : une demande ordinaire par heure
    /// ([`COMPTA_ABSENTE`]), « Relire » comme d'habitude.
    pub fn vouloir(&mut self, relire: bool, maintenant: Instant, a_relire: impl FnOnce() -> bool) {
        if self.occupee {
            return;
        }
        if self.absente && !relire && !self.partie_a.is_some_and(|t| maintenant + COMPTA_MARGE >= t + COMPTA_ABSENTE) {
            return;
        }
        let quand = match self.partie_a {
            Some(t) if relire => (t + COMPTA_RELIRE).max(maintenant),
            Some(t) if maintenant + COMPTA_MARGE >= t + COMPTA_INTERVALLE => maintenant,
            Some(_) => return,
            None => {
                if !relire && !a_relire() {
                    return;
                }
                maintenant
            }
        };
        // Une demande attend déjà : la plus proche des deux.
        self.voulue = Some(match self.voulue {
            Some((t, r)) => (t.min(quand), r || relire),
            None => (quand, relire),
        });
    }

    /// Au battement, la connexion le permettant (le jeton de l'émission `emis`, valable) : la lecture part-elle ? Oui :
    /// `Some` (« Relire » ou non), et la voilà en cours. Jamais avec un jeton qu'une lecture a vu refusé : la demande
    /// attend le suivant.
    pub fn partir(&mut self, maintenant: Instant, emis: u64) -> Option<bool> {
        let (quand, relire) = self.voulue?;
        if self.occupee || maintenant < quand || self.refuse == Some(emis) {
            return None;
        }
        self.voulue = None;
        self.occupee = true;
        self.partie_a = Some(maintenant);
        Some(relire)
    }

    /// La lecture a fini ([`FinDeLecture`]) : un jeton refusé fait revenir la demande, qui attend le jeton suivant ; un
    /// serveur sans les outils n'est plus relu qu'une fois par heure, ou sur « Relire », jusqu'à une lecture qui
    /// réussit.
    pub fn finie(&mut self, fin: FinDeLecture, relire: bool, maintenant: Instant) {
        self.occupee = false;
        match fin {
            FinDeLecture::Lue => self.absente = false,
            FinDeLecture::Manquee => {}
            FinDeLecture::JetonRefuse(emis) => {
                self.refuse = Some(emis);
                self.voulue = Some((maintenant, relire));
            }
            FinDeLecture::Absente => self.absente = true,
        }
    }

    /// La connexion s'en va : plus rien n'attend, et une lecture en route sera écartée (son tour est passé).
    pub fn oublier(&mut self) {
        *self = DemandesDeLaCompta::default();
    }

    /// Une demande attend-elle son moment ?
    pub fn voulue(&self) -> bool {
        self.voulue.is_some()
    }
}

/// Ce que le chef sait de sa connexion à Constructo AI, pour la comptabilité : reliée (l'espace de Constructo AI, une
/// connexion faite), un renouvellement en cours, le jeton d'accès valable et pas encore à renouveler, son émission, et
/// le tour de connexion (une déconnexion le fait passer).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EtatDeLaConnexion {
    pub reliee: bool,
    pub occupee: bool,
    pub acces_valable: bool,
    pub a_renouveler: bool,
    pub emis: u64,
    pub tour: u64,
}

/// Ce que [`LaCompta`] demande au chef : charger `registre.json` dans un fil (pour le tour `tour`) ; l'écrire, par son
/// fil ([`EcrivainDuRegistre`]) ; lire la comptabilité dans un fil, avec le jeton du moment ; faire renouveler le jeton
/// de l'émission `emis`, refusé ; une ligne au journal.
#[derive(Clone, Debug, PartialEq)]
pub enum PourLaCompta {
    Charger { tour: u64 },
    Ecrire(FichierDuRegistre),
    Lire { tour: u64, emis: u64, relire: bool },
    Renouveler { emis: u64 },
    Noter(String),
}

/// La comptabilité de l'ERP telle que le chef la mène, pure : l'heure et sa connexion arrivent en argument, ce qu'il y
/// a à faire en sort ([`PourLaCompta`]) ; le chef le fait. Elle tient ce que dit `registre.json` (`None` : pas encore
/// chargé ; il se charge dans un fil à la première demande — vingt mille factures se lisent en un dixième de seconde,
/// que le fil du chef ne passe pas à attendre), le chargement en route (son tour), la demande qui l'attend (« Relire »
/// ou non), et les demandes ([`DemandesDeLaCompta`]). Ses gardes sont essayées une à une (la QA5 du lot 13.2 : quatre
/// gardes du chef retirées ensemble laissaient les essais verts) : une lecture d'avant la déconnexion écartée, une
/// lecture qui ne part qu'avec un jeton prêt, la déconnexion qui efface tout, un jeton refusé qui fait renouveler.
#[derive(Clone, Debug, Default)]
pub struct LaCompta {
    fichier: Option<FichierDuRegistre>,
    chargement: Option<u64>,
    en_attente: Option<bool>,
    demandes: DemandesDeLaCompta,
}

impl LaCompta {
    /// Plus de connexion (une déconnexion, ou Eve lancée sans elle) : `registre.json` ne garde rien de l'entreprise —
    /// ce qui restait s'efface ; un fichier qui ne dit déjà que cela ne se réécrit pas (`rien_a_effacer` ne se demande
    /// que s'il n'est pas chargé). Plus aucune demande n'attend, et un chargement en route sera écarté.
    pub fn sans_connexion(&mut self, rien_a_effacer: impl FnOnce() -> bool) -> Vec<PourLaCompta> {
        self.demandes.oublier();
        self.chargement = None;
        self.en_attente = None;
        let deja = match &self.fichier {
            Some(f) => *f == FichierDuRegistre::non_connecte(),
            None => rien_a_effacer(),
        };
        self.fichier = Some(FichierDuRegistre::non_connecte());
        if deja { Vec::new() } else { vec![PourLaCompta::Ecrire(FichierDuRegistre::non_connecte())] }
    }

    /// L'onglet la demande (`relire` : « Relire », « Réessayer » ; `heure` : celle du poste, « AAAA-MM-JJ HH:MM:SS ») :
    /// [`DemandesDeLaCompta::vouloir`]. Sans connexion, rien. `registre.json` pas encore chargé : il se charge, une
    /// fois, et la demande l'attend.
    pub fn demande(
        &mut self,
        relire: bool,
        maintenant: Instant,
        heure: &str,
        c: &EtatDeLaConnexion,
    ) -> Vec<PourLaCompta> {
        if !c.reliee {
            return Vec::new();
        }
        if let Some(f) = &self.fichier {
            self.demandes.vouloir(relire, maintenant, || f.a_relire(heure));
            return Vec::new();
        }
        self.en_attente = Some(self.en_attente.unwrap_or(false) || relire);
        if self.chargement.is_some() {
            return Vec::new();
        }
        self.chargement = Some(c.tour);
        vec![PourLaCompta::Charger { tour: c.tour }]
    }

    /// `registre.json` est chargé, dans son fil (`tour` : la connexion qui l'a demandé) : la demande qui l'attendait se
    /// fait. Un chargement d'avant une déconnexion (son tour passé, ou le fichier déjà effacé) est écarté.
    pub fn charge(
        &mut self,
        f: FichierDuRegistre,
        tour: u64,
        maintenant: Instant,
        heure: &str,
        c: &EtatDeLaConnexion,
    ) -> Vec<PourLaCompta> {
        if self.chargement != Some(tour) || tour != c.tour || self.fichier.is_some() {
            return vec![PourLaCompta::Noter(
                "comptabilité : un registre.json chargé avant la déconnexion, écarté".into(),
            )];
        }
        self.chargement = None;
        let attendue = self.en_attente.take();
        let f = self.fichier.insert(f);
        if let Some(relire) = attendue
            && c.reliee
        {
            self.demandes.vouloir(relire, maintenant, || f.a_relire(heure));
        }
        Vec::new()
    }

    /// Au battement. Sans connexion, ce qui restait s'efface (la déconnexion l'a fait ; le battement le garantit). Avec
    /// elle, la lecture que l'onglet a demandée part quand son moment est venu — jamais pendant un renouvellement, ni
    /// avec un jeton qui ne vaut plus ou arrive à échéance, ni avec un jeton déjà refusé ; `registre.json` dit qu'elle
    /// commence, la lecture d'avant gardée.
    pub fn battement(&mut self, maintenant: Instant, c: &EtatDeLaConnexion) -> Vec<PourLaCompta> {
        if !c.reliee {
            let a_effacer = self.fichier.as_ref().is_some_and(|f| *f != FichierDuRegistre::non_connecte());
            if a_effacer || self.demandes.voulue() || self.chargement.is_some() || self.en_attente.is_some() {
                return self.sans_connexion(|| false);
            }
            return Vec::new();
        }
        if !self.demandes.voulue() || c.occupee || !c.acces_valable || c.a_renouveler {
            return Vec::new();
        }
        let Some(relire) = self.demandes.partir(maintenant, c.emis) else { return Vec::new() };
        let en_cours = self.fichier.get_or_insert_with(FichierDuRegistre::default).en_cours();
        self.fichier = Some(en_cours.clone());
        vec![PourLaCompta::Ecrire(en_cours), PourLaCompta::Lire { tour: c.tour, emis: c.emis, relire }]
    }

    /// Une lecture a fini, dans son fil (`tour`, `emis`, `relire` : ce qu'elle était ; `lu_le` : l'heure du poste,
    /// « AAAA-MM-JJTHH:MM:SS »). Une lecture d'avant la déconnexion est écartée : jamais la comptabilité d'une autre
    /// connexion dans `registre.json`. Réussie, `registre.json` la prend (`a_jour`) ; manquée, il garde la dernière et
    /// dit pourquoi — un refus d'accès ([`refus_d_autorisation`]) n'en laisse rien. Un jeton refusé fait renouveler, et
    /// la demande attend le suivant ; un serveur sans les outils n'est plus relu que sur « Relire ». Au journal, une
    /// ligne : sa durée et des nombres.
    #[allow(clippy::too_many_arguments)]
    pub fn lue(
        &mut self,
        tour: u64,
        emis: u64,
        relire: bool,
        duree: Duration,
        issue: Result<LectureDeLaCompta, Panne>,
        maintenant: Instant,
        lu_le: &str,
        c: &EtatDeLaConnexion,
    ) -> Vec<PourLaCompta> {
        if tour != c.tour || !c.reliee {
            return vec![PourLaCompta::Noter("comptabilité : une lecture d'avant la déconnexion, écartée".into())];
        }
        let demande = if relire { " (« Relire »)" } else { "" };
        match issue {
            Ok(l) => {
                self.demandes.finie(FinDeLecture::Lue, relire, maintenant);
                let mut ordres = vec![PourLaCompta::Noter(format!("comptabilité{demande} : {}", l.resume()))];
                let f = FichierDuRegistre::lu(l.registre, lu_le);
                self.fichier = Some(f.clone());
                ordres.push(PourLaCompta::Ecrire(f));
                if l.jeton_refuse {
                    ordres.push(PourLaCompta::Renouveler { emis });
                }
                ordres
            }
            Err(p) => {
                let fin = match p {
                    Panne::Jeton => FinDeLecture::JetonRefuse(emis),
                    Panne::Inconnu(_) => FinDeLecture::Absente,
                    _ => FinDeLecture::Manquee,
                };
                self.demandes.finie(fin, relire, maintenant);
                let quoi =
                    if p == Panne::Jeton { "Constructo AI refuse le jeton d'accès".to_string() } else { p.texte() };
                let phrase = phrase_de_la_compta(&p);
                let f = if refus_d_autorisation(&p) {
                    FichierDuRegistre::refusee(&phrase)
                } else {
                    self.fichier.get_or_insert_with(FichierDuRegistre::default).manque(&phrase)
                };
                self.fichier = Some(f.clone());
                let note = format!("comptabilité{demande} : lecture manquée en {} — {quoi}", secondes(duree));
                let mut ordres = vec![PourLaCompta::Noter(note), PourLaCompta::Ecrire(f)];
                if p == Panne::Jeton {
                    ordres.push(PourLaCompta::Renouveler { emis });
                }
                ordres
            }
        }
    }
}

// ---- Le client MCP -------------------------------------------------------------------------------------------------

/// Une session MCP au connecteur de Constructo AI.
struct Client {
    url: String,
    jeton: String,
    /// La session que le serveur a ouverte (`Mcp-Session-Id`), et la version du protocole qu'il a dite.
    session: Option<String>,
    version: Option<String>,
    /// Le numéro de la prochaine requête JSON-RPC.
    suivant: u64,
    /// L'échéance de toute la lecture (la comptabilité : [`COMPTA_DUREE_MAX`]) : chaque requête n'a que ce qui en
    /// reste.
    echeance: Option<Instant>,
}

impl Client {
    fn ouvrir(url: &str, jeton: &str, echeance: Option<Instant>) -> Result<Client, Panne> {
        let mut c = Client {
            url: url.trim_end_matches('/').to_string(),
            jeton: jeton.to_string(),
            session: None,
            version: None,
            suivant: 1,
            echeance,
        };
        c.initialiser()?;
        Ok(c)
    }

    /// `initialize`, puis `notifications/initialized` : le serveur refuse toute requête avant.
    fn initialiser(&mut self) -> Result<(), Panne> {
        self.session = None;
        self.version = None;
        let params = json!({
            "protocolVersion": VERSION_MCP,
            "capabilities": {},
            "clientInfo": { "name": NOM_CLIENT, "version": env!("CARGO_PKG_VERSION") },
        });
        let (reponse, message) = self.demander("initialize", params, DELAI_REVEIL)?;
        if let Some(e) = message.get("error") {
            return Err(Panne::Serveur(format!("Constructo AI refuse la session ({})", erreur_jsonrpc(e))));
        }
        self.session = reponse.entete("mcp-session-id").filter(|s| session_valable(s)).map(str::to_string);
        let version = message.pointer("/result/protocolVersion").and_then(Value::as_str).filter(|v| version_valable(v));
        self.version = Some(version.unwrap_or(VERSION_MCP).to_string());
        self.notifier("notifications/initialized")
    }

    /// Les en-têtes du protocole : ce qu'Eve accepte, la session, la version.
    fn entetes(&self) -> Vec<(&'static str, String)> {
        let mut e = vec![("Accept", "application/json, text/event-stream".to_string())];
        if let Some(s) = &self.session {
            e.push(("Mcp-Session-Id", s.clone()));
        }
        if let Some(v) = &self.version {
            e.push(("MCP-Protocol-Version", v.clone()));
        }
        e
    }

    /// Un message au serveur (un POST). La seule redirection prise : « /mcp » vers « /mcp/ » ([`barre_finale`]).
    fn poster(&mut self, message: &Value, delai: Duration) -> Result<https::Reponse, Panne> {
        let corps = message.to_string().into_bytes();
        let echeance = self.echeance;
        // Une requête qui manque à l'échéance — à un quart de seconde près : la minuterie de WinHTTP n'est pas plus
        // juste — manque d'elle, pas du serveur.
        let a_l_echeance = || echeance.is_some_and(|e| Instant::now() + Duration::from_millis(250) >= e);
        for _ in 0..2 {
            // Une lecture qui a son échéance : la requête n'en prend que ce qui reste ; plus rien, elle ne part pas.
            let delai = match echeance {
                Some(e) => match e.saturating_duration_since(Instant::now()) {
                    reste if reste.is_zero() => return Err(Panne::Serveur(LECTURE_TROP_LONGUE.into())),
                    reste => delai.min(reste),
                },
                None => delai,
            };
            let entetes = self.entetes();
            let entetes: Vec<(&str, &str)> = entetes.iter().map(|(n, v)| (*n, v.as_str())).collect();
            let r = https::requete("POST", &self.url, &self.jeton, &entetes, Some(("application/json", &corps)), delai)
                .map_err(|e| match a_l_echeance() {
                    true => Panne::Serveur(LECTURE_TROP_LONGUE.into()),
                    false => Panne::Serveur(format!("Constructo AI ne répond pas : {e}")),
                })?;
            if matches!(r.statut, 307 | 308)
                && let Some(ailleurs) = r.entete("location").and_then(|l| barre_finale(&self.url, l))
            {
                self.url = ailleurs;
                continue;
            }
            // Un 401, le jeton : le chef en décide. Un 403 refuse l'accès lui-même : un jeton neuf n'y changerait rien.
            return match r.statut {
                200..=299 => Ok(r),
                401 => Err(Panne::Jeton),
                403 => Err(Panne::Interdit),
                404 if self.session.is_some() => Err(Panne::Session),
                s => Err(Panne::Serveur(raison_http(s))),
            };
        }
        Err(Panne::Serveur("Constructo AI renvoie la demande ailleurs".into()))
    }

    /// Une requête JSON-RPC : la réponse HTTP et le message qui lui répond (son `result` ou son `error`).
    fn demander(&mut self, methode: &str, params: Value, delai: Duration) -> Result<(https::Reponse, Value), Panne> {
        let id = self.suivant;
        self.suivant += 1;
        let r = self.poster(&json!({ "jsonrpc": "2.0", "id": id, "method": methode, "params": params }), delai)?;
        let message = message_de_la_reponse(r.entete("content-type"), &r.corps, id).map_err(Panne::Serveur)?;
        Ok((r, message))
    }

    /// Une notification JSON-RPC (sans réponse : le serveur dit 202).
    fn notifier(&mut self, methode: &str) -> Result<(), Panne> {
        self.poster(&json!({ "jsonrpc": "2.0", "method": methode }), DELAI_APPEL).map(|_| ())
    }

    /// Un outil, et son texte. Un outil hors de la liste ne part jamais ; une session que le serveur a oubliée se
    /// rouvre, une fois.
    fn appeler(&mut self, outil: &str, arguments: Value) -> Result<String, Panne> {
        if !permis(outil) {
            return Err(Panne::Refuse(outil.to_string()));
        }
        let params = json!({ "name": outil, "arguments": arguments });
        let message = match self.demander("tools/call", params.clone(), DELAI_APPEL) {
            Err(Panne::Session) => {
                self.initialiser()?;
                self.demander("tools/call", params, DELAI_APPEL)?.1
            }
            r => r?.1,
        };
        if let Some(e) = message.get("error") {
            if outil_inconnu(e["message"].as_str().unwrap_or("")) {
                return Err(Panne::Inconnu(outil.to_string()));
            }
            return Err(Panne::Outil(format!("{outil} : {}", erreur_jsonrpc(e))));
        }
        let resultat = &message["result"];
        let texte = texte_du_resultat(resultat);
        if resultat["isError"].as_bool() == Some(true) {
            if outil_inconnu(&texte) {
                return Err(Panne::Inconnu(outil.to_string()));
            }
            return Err(Panne::Outil(format!("{outil} : {}", ecourter(&sans_porteur(&texte), 200))));
        }
        Ok(texte)
    }

    /// Ferme la session (DELETE) : le serveur la garderait en mémoire. Sans réponse, tant pis.
    fn fermer(self) {
        let Some(session) = &self.session else { return };
        let mut entetes = vec![("Mcp-Session-Id", session.as_str())];
        if let Some(v) = &self.version {
            entetes.push(("MCP-Protocol-Version", v.as_str()));
        }
        let _ = https::requete("DELETE", &self.url, &self.jeton, &entetes, None, Duration::from_secs(10));
    }
}

/// Un identifiant de session MCP : des caractères visibles seulement (la spécification), et court.
fn session_valable(s: &str) -> bool {
    !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| b.is_ascii_graphic())
}

/// Une version du protocole : « 2025-06-18 ».
fn version_valable(v: &str) -> bool {
    !v.is_empty() && v.len() <= 32 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
}

/// Ce que dit un code HTTP d'échec.
fn raison_http(statut: u16) -> String {
    match statut {
        502..=504 => {
            format!("Constructo AI ne répond pas encore (HTTP {statut} : le serveur se réveille ou redémarre)")
        }
        429 => "Constructo AI demande d'attendre (HTTP 429)".into(),
        403 => "Constructo AI refuse l'accès à ces lectures (HTTP 403)".into(),
        s => format!("Constructo AI a répondu HTTP {s}"),
    }
}

/// L'erreur d'un message JSON-RPC, courte (ce qu'en dit le serveur, sans jeton possible).
fn erreur_jsonrpc(e: &Value) -> String {
    let m = e["message"].as_str().unwrap_or("erreur");
    let code = e["code"].as_i64().map(|c| format!(" ({c})")).unwrap_or_default();
    ecourter(&sans_porteur(&format!("{m}{code}")), 200)
}

/// Le texte d'un résultat d'outil : ses morceaux de texte, réunis.
fn texte_du_resultat(r: &Value) -> String {
    r["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["type"] == "text")
        .filter_map(|c| c["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// La redirection de Starlette (`Mount("/mcp")` : « /mcp » vers « /mcp/ », un 307) : la même adresse avec sa barre
/// finale, sur le même serveur. Eve ne suit aucune autre redirection avec son jeton ; celle-là, elle la prend chez
/// elle, en gardant son https (le serveur, derrière Render, peut la dire en http).
fn barre_finale(url: &str, location: &str) -> Option<String> {
    let (_, apres) = url.split_once("://")?;
    let (hote, chemin) = apres.split_once('/')?;
    let voulu = format!("/{chemin}/");
    let sans_port = |h: &str| h.rsplit_once(':').map_or(h, |(nom, _)| nom).to_ascii_lowercase();
    let pareille = match location.split_once("://") {
        None => location == voulu,
        Some((_, l)) => {
            let (h, c) = l.split_once('/')?;
            sans_port(h) == sans_port(hote) && format!("/{c}") == voulu
        }
    };
    pareille.then(|| format!("{url}/"))
}

/// Le message JSON-RPC qui répond à la requête `id`, dans une réponse en `application/json` ou en `text/event-stream`
/// (le serveur peut y glisser des notifications avant elle, et répondre en lot).
pub fn message_de_la_reponse(sorte: Option<&str>, corps: &[u8], id: u64) -> Result<Value, String> {
    let texte = String::from_utf8_lossy(corps);
    let sse = sorte.is_some_and(|s| s.trim().to_ascii_lowercase().starts_with("text/event-stream"));
    let messages: Vec<Value> = if sse {
        evenements_sse(&texte).iter().filter_map(|d| serde_json::from_str(d).ok()).collect()
    } else {
        vec![serde_json::from_str(texte.trim()).map_err(|_| "la réponse de Constructo AI ne se lit pas".to_string())?]
    };
    messages
        .into_iter()
        .flat_map(|m| match m {
            Value::Array(lot) => lot,
            m => vec![m],
        })
        .find(|m| m["id"].as_u64() == Some(id) && (m.get("result").is_some() || m.get("error").is_some()))
        .ok_or_else(|| "Constructo AI n'a pas répondu à la demande".to_string())
}

/// Les données des événements d'un flux SSE : les lignes `data:` d'un même événement, réunies. Un événement finit à
/// une ligne vide, ou avec le flux ; les fins de ligne sont `\r\n`, `\n` ou `\r` ; les commentaires (`: ping`),
/// `event:`, `id:` et `retry:` ne portent rien de plus pour Eve.
fn evenements_sse(texte: &str) -> Vec<String> {
    let texte = texte.replace("\r\n", "\n").replace('\r', "\n");
    let mut evenements = Vec::new();
    let mut donnees: Vec<&str> = Vec::new();
    for ligne in texte.split('\n') {
        if ligne.is_empty() {
            if !donnees.is_empty() {
                evenements.push(donnees.join("\n"));
                donnees.clear();
            }
        } else if let Some(d) = ligne.strip_prefix("data:") {
            donnees.push(d.strip_prefix(' ').unwrap_or(d));
        }
    }
    if !donnees.is_empty() {
        evenements.push(donnees.join("\n"));
    }
    evenements
}

// ---- Ce que rendent les outils -------------------------------------------------------------------------------------

/// Le JSON d'un outil : tel quel (`tableau_kpis`), ou après sa ligne d'en-tête (« 3 projet(s) trouve(s): »). Un texte
/// d'erreur (« Erreur lors de la lecture des KPIs (voir logs serveur). ») n'en a pas.
fn json_de(texte: &str) -> Option<Value> {
    let t = texte.trim();
    if t.starts_with(['{', '[']) {
        return serde_json::from_str(t).ok();
    }
    let (tete, reste) = t.split_once('\n')?;
    if !tete.trim_end().ends_with(':') {
        return None;
    }
    serde_json::from_str(reste.trim()).ok()
}

fn objet(texte: &str) -> Option<Value> {
    json_de(texte).filter(Value::is_object)
}

/// Une liste : son JSON, ou rien du tout — « 0 bon(s) de commande trouvé(s): » suivi de « Aucun(e) bon de commande
/// trouvé(e). » (le `_format_results` du serveur) est une liste vide.
fn liste(texte: &str) -> Option<Vec<Value>> {
    if let Some((tete, reste)) = texte.trim().split_once('\n')
        && tete.trim_end().ends_with(':')
        && reste.trim_start().starts_with("Aucun")
    {
        return Some(Vec::new());
    }
    match json_de(texte)? {
        Value::Array(l) => Some(l),
        _ => None,
    }
}

/// Un compte, entier et positif ; le serveur en rend parfois un en virgule flottante.
fn entier(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_f64().filter(|f| f.is_finite() && *f >= 0.0).map(|f| f.round() as u64))
}

/// Un montant fini (ou un texte qui en est un : le `default=str` du serveur).
fn montant(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).filter(|f: &f64| f.is_finite())
}

/// Un montant plus grand que zéro (un budget à 0, c'est un budget qui manque).
fn positif(v: &Value) -> Option<f64> {
    montant(v).filter(|m| *m > 0.0)
}

/// Un texte court d'une ligne : un nom, une note.
fn texte(v: &Value) -> Option<String> {
    let t = v.as_str()?.split_whitespace().collect::<Vec<_>>().join(" ");
    (!t.is_empty()).then(|| ecourter(&t, TEXTE_MAX))
}

/// Une date (« 2026-10-26 » ; le début d'une date ISO).
fn date(v: &Value) -> Option<String> {
    let d = v.as_str()?.get(..10)?;
    numero_du_jour(d).map(|_| d.to_string())
}

/// Une note, courte : une ligne de carte.
fn court(v: &Value) -> Option<String> {
    texte(v).map(|t| ecourter(&t, 120))
}

/// Les dates d'abord, de la plus proche à la plus lointaine ; sans date, ensuite.
fn ordre_des_dates(a: &Option<String>, b: &Option<String>) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Deux décimales : des heures.
fn arrondi(h: f64) -> f64 {
    (h * 100.0).round() / 100.0
}

/// Les heures entre deux pointages ISO (« 2026-10-26T07:25:00 ») ; rien s'ils ne se lisent pas, ou plus d'une journée.
fn duree_en_heures(debut: &str, fin: &str) -> Option<f64> {
    let s = |t: &str| crate::rappels::en_secondes(&t.get(..19)?.replace('T', " "));
    let h = (s(fin)? - s(debut)?) as f64 / 3600.0;
    (h > 0.0 && h <= 24.0).then_some(h)
}

/// Les heures de cette semaine et de la précédente, d'après leurs pointages (`lister_pointages` : `punch_in`,
/// `punch_out`, `total_hours`, l'employé, le projet ; jamais un taux) : par employé, par jour, par projet (`projets` :
/// le nom des projets en cours, par leur numéro dans l'ERP) ; ce qui est saisi pour des jours à venir ; qui n'a rien le
/// dernier jour de semaine avant aujourd'hui.
fn heures(pointages: &[Value], aujourdhui: &str, projets: &HashMap<u64, (String, Option<String>)>) -> Heures {
    #[derive(Default)]
    struct Cumul {
        cette_semaine: f64,
        semaine_passee: f64,
        a_venir: f64,
        au_travail: bool,
        jours: BTreeMap<String, f64>,
    }
    let n = numero_du_jour(aujourdhui).unwrap_or(0);
    let lundi = n - (n + 3).rem_euclid(7);
    let (cette, passee) = (date_du_numero(lundi), date_du_numero(lundi - 7));
    // Le dernier jour de semaine avant aujourd'hui : un lundi, le vendredi d'avant.
    let mut veille = n - 1;
    while (veille + 3).rem_euclid(7) >= 5 {
        veille -= 1;
    }
    let veille = date_du_numero(veille);
    let mut h = Heures { semaine: cette.clone(), ..Heures::default() };
    let mut employes: BTreeMap<String, Cumul> = BTreeMap::new();
    let mut par_projet: HashMap<Option<u64>, f64> = HashMap::new();
    for p in pointages {
        let (Some(debut), Some(jour)) = (p["punch_in"].as_str(), date(&p["punch_in"])) else { continue };
        if jour < passee {
            continue;
        }
        let nom = [texte(&p["prenom"]), texte(&p["nom_employe"])].into_iter().flatten().collect::<Vec<_>>().join(" ");
        let nom = if nom.is_empty() { "Employé inconnu".to_string() } else { nom };
        let ouvert = p["punch_out"].as_str().is_none_or(str::is_empty);
        let duree = montant(&p["total_hours"])
            .filter(|d| *d > 0.0)
            .or_else(|| duree_en_heures(debut, p["punch_out"].as_str()?))
            .unwrap_or(0.0);
        let c = employes.entry(nom).or_default();
        if jour.as_str() > aujourdhui {
            c.a_venir += duree;
            h.a_venir += duree;
            continue;
        }
        if jour >= cette {
            c.cette_semaine += duree;
            h.cette_semaine += duree;
            *par_projet.entry(entier(&p["project_id"])).or_default() += duree;
        } else {
            c.semaine_passee += duree;
            h.semaine_passee += duree;
        }
        if duree > 0.0 {
            *c.jours.entry(jour.clone()).or_default() += duree;
        }
        c.au_travail |= ouvert && jour == aujourdhui;
        let saisie = debut.get(..16).map(str::to_string);
        if saisie > h.derniere_saisie {
            h.derniere_saisie = saisie;
        }
    }
    h.employes_actifs = employes.len() as u64;
    let sans_heures: Vec<String> = employes
        .iter()
        .filter(|(_, c)| c.cette_semaine + c.semaine_passee > 0.0 && c.jours.get(&veille).is_none_or(|d| *d <= 0.0))
        .map(|(nom, _)| nom.clone())
        .collect();
    h.manquantes = Some(Manquantes { jour: veille, employes: sans_heures });
    h.employes = employes
        .into_iter()
        .take(EMPLOYES_MAX)
        .map(|(nom, c)| HeuresEmploye {
            nom,
            cette_semaine: arrondi(c.cette_semaine),
            semaine_passee: arrondi(c.semaine_passee),
            a_venir: arrondi(c.a_venir),
            au_travail: c.au_travail,
            jours: c.jours.into_iter().map(|(jour, d)| HeuresJour { jour, heures: arrondi(d) }).collect(),
        })
        .collect();
    let mut projets_de_la_semaine: Vec<HeuresProjet> = par_projet
        .into_iter()
        .filter(|(_, d)| *d > 0.0)
        .map(|(id, d)| {
            let (nom, numero) = match id {
                None => ("Sans projet".to_string(), None),
                Some(id) => projets.get(&id).cloned().unwrap_or_else(|| ("Autre projet".to_string(), None)),
            };
            HeuresProjet { nom, numero, heures: arrondi(d) }
        })
        .collect();
    projets_de_la_semaine.sort_by(|a, b| b.heures.total_cmp(&a.heures).then_with(|| a.nom.cmp(&b.nom)));
    projets_de_la_semaine.truncate(PAR_PROJET_MAX);
    h.par_projet = projets_de_la_semaine;
    (h.cette_semaine, h.semaine_passee, h.a_venir) =
        (arrondi(h.cette_semaine), arrondi(h.semaine_passee), arrondi(h.a_venir));
    h
}

/// Les projets d'une liste de `lister_projets` (la ligne entière de la table des projets, ses champs sensibles ôtés par
/// le serveur) : ceux qui ont un nom.
fn projets_de(l: &[Value]) -> Vec<Projet> {
    l.iter()
        .filter_map(|p| {
            Some(Projet {
                id: entier(&p["id"]),
                nom: texte(&p["nom_projet"])?,
                numero: texte(&p["numero_projet"]),
                client: texte(&p["client_nom_cache"]).or_else(|| texte(&p["client_nom_direct"])),
                statut: texte(&p["statut"]),
                // `budget_total` : la colonne canonique de l'ERP ; `prix_estime`, l'ancienne.
                valeur: positif(&p["budget_total"]).or_else(|| positif(&p["prix_estime"])),
                ville: texte(&p["ville_chantier"]),
                debut: date(&p["date_debut_reel"]),
                fin: date(&p["date_fin_reel"]),
                adresse: texte(&p["adresse_chantier"]),
            })
        })
        .collect()
}

fn noter(manques: &mut Vec<String>, outil: &str) {
    if !manques.iter().any(|m| m == outil) {
        manques.push(outil.to_string());
    }
}

/// Les textes qu'un outil a rendus, dans l'ordre de ses appels (`None` : l'appel a flanché).
fn rendus<'a>(reponses: &'a [(&str, Result<String, String>)], outil: &str) -> Vec<Option<&'a str>> {
    reponses.iter().filter(|(o, _)| *o == outil).map(|(_, r)| r.as_ref().ok().map(String::as_str)).collect()
}

/// Ce que les outils ont rendu (leur texte, ou leur erreur), analysé : les données du tableau de bord, et les outils
/// qui n'ont rien donné de lisible.
fn analyser(reponses: &[(&str, Result<String, String>)], aujourdhui: &str) -> (Entreprise, Vec<String>) {
    let un = |outil: &str| rendus(reponses, outil).into_iter().next().flatten();
    let mut e = Entreprise::default();
    let mut manques = Vec::new();

    match un("tableau_kpis").and_then(objet) {
        Some(v) => {
            let k = &mut e.kpis;
            k.projets_en_cours = entier(&v["projets_en_cours"]);
            k.a_recevoir = montant(&v["factures_solde_du"]);
            k.pipeline = montant(&v["pipeline_valeur"]);
            k.opportunites_ouvertes = entier(&v["opportunites_ouvertes"]);
            if entier(&v["periode_jours"]) == Some(30) {
                k.encaissements_30_jours = montant(&v["revenu_encaisse_periode"]);
            }
            k.produits_sous_seuil = entier(&v["stock_critique"]);
            k.clients_actifs = entier(&v["clients_actifs"]);
            k.employes_actifs = entier(&v["employes_actifs"]);
            k.taux_conversion_devis = montant(&v["devis_taux_conversion_pct"]);
        }
        None => noter(&mut manques, "tableau_kpis"),
    }

    match un("tableau_alertes").and_then(objet) {
        Some(v) => {
            e.kpis.factures_en_retard = entier(&v["factures_en_retard"]["nb"]);
            e.kpis.en_retard = montant(&v["factures_en_retard"]["montant"]);
            if let Some(n) = entier(&v["produits_sous_seuil"]) {
                e.kpis.produits_sous_seuil = Some(n);
            }
        }
        None => noter(&mut manques, "tableau_alertes"),
    }

    match un("tableau_factures_agees").and_then(objet).filter(|v| v["tranches"].is_array()) {
        Some(v) => {
            let mut b = BalanceAgee::default();
            for t in v["tranches"].as_array().into_iter().flatten() {
                let m = montant(&t["montant"]).unwrap_or(0.0);
                match t["tranche"].as_str().unwrap_or("") {
                    "a_venir" => b.a_venir += m,
                    "0_30" => b.de_0_a_30 += m,
                    "31_60" => b.de_31_a_60 += m,
                    "61_90" => b.de_61_a_90 += m,
                    "90_plus" => b.plus_de_90 += m,
                    _ => {}
                }
            }
            let somme = b.a_venir + b.de_0_a_30 + b.de_31_a_60 + b.de_61_a_90 + b.plus_de_90;
            b.total = montant(&v["total_du"]).unwrap_or(somme);
            e.balance_agee = Some(b);
            e.factures_en_retard = v["factures_en_retard"].as_array().map(|l| {
                l.iter()
                    .filter_map(|f| {
                        Some(FactureEnRetard {
                            numero: texte(&f["numero_facture"]),
                            client: texte(&f["client_nom"]),
                            solde: montant(&f["solde_du"])?,
                            jours: entier(&f["jours_retard"]),
                            echeance: date(&f["date_echeance"]),
                        })
                    })
                    .take(FACTURES_MAX)
                    .collect()
            });
        }
        None => noter(&mut manques, "tableau_factures_agees"),
    }

    // Les projets en cours (le premier appel), puis à venir (le second, pour le planning), et leur nom par leur numéro
    // dans l'ERP : les pointages, les factures, les livraisons et les bons de travail ne donnent que celui-ci.
    let mut noms_des_projets: HashMap<u64, (String, Option<String>)> = HashMap::new();
    let projets_lus: Vec<Option<Vec<Projet>>> =
        rendus(reponses, "lister_projets").into_iter().map(|t| t.and_then(liste).map(|l| projets_de(&l))).collect();
    for p in projets_lus.iter().flatten().flatten() {
        if let Some(id) = p.id {
            noms_des_projets.insert(id, (p.nom.clone(), p.numero.clone()));
        }
    }
    match projets_lus.first().cloned().flatten() {
        Some(projets) => {
            e.kpis.valeur_projets_en_cours = Some(projets.iter().filter_map(|p| p.valeur).sum());
            e.projets = Some(projets.into_iter().take(PROJETS_MAX).collect());
        }
        None => noter(&mut manques, "lister_projets"),
    }
    match projets_lus.get(1) {
        Some(Some(projets)) => e.projets_en_attente = Some(projets.iter().take(PROJETS_MAX).cloned().collect()),
        Some(None) => noter(&mut manques, "lister_projets"),
        None => {}
    }
    let nom_du_projet = |v: &Value| entier(v).and_then(|id| noms_des_projets.get(&id)).map(|(nom, _)| nom.clone());

    match un("tableau_pipeline_ventes").and_then(objet).filter(|v| v["par_statut"].is_array()) {
        Some(v) => {
            e.pipeline = Some(
                v["par_statut"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| {
                        let etape = texte(&p["statut"])?;
                        let valeur = montant(&p["valeur"]).unwrap_or(0.0);
                        (!vente_fermee(&etape)).then(|| Etape { etape, nombre: entier(&p["nb"]).unwrap_or(0), valeur })
                    })
                    .collect(),
            );
        }
        None => noter(&mut manques, "tableau_pipeline_ventes"),
    }

    // Les 50 devis les plus récents : en cours, les statuts d'avant la décision du client (`DEVIS_STATUSES` de l'ERP) ;
    // en attente d'une réponse, « Envoye » et « En attente » (`routers/devis.py`, le compte « en attente »).
    match un("lister_devis").and_then(liste) {
        Some(l) => {
            let tous: Vec<Devis> = l
                .iter()
                .filter_map(|d| {
                    Some(Devis {
                        id: entier(&d["id"]),
                        numero: texte(&d["numero_devis"]),
                        nom: texte(&d["nom_projet"])?,
                        client: texte(&d["client_nom_cache"]).or_else(|| texte(&d["client_nom_direct"])),
                        statut: texte(&d["statut"]).unwrap_or_default(),
                        // `investissement_total` : le montant que somme l'ERP pour ses devis.
                        valeur: positif(&d["investissement_total"]).or_else(|| positif(&d["prix_estime"])),
                        depuis: date(&d["date_soumis"]).or_else(|| date(&d["created_at"])),
                        modifie: date(&d["updated_at"]).or_else(|| date(&d["created_at"])),
                        debut: date(&d["date_prevu"]),
                        fin: date(&d["date_fin"]),
                    })
                })
                .collect();
            let statut = |d: &Devis| normaliser(&d.statut);
            let mut devis: Vec<Devis> =
                tous.iter().filter(|d| matches!(statut(d).as_str(), "envoye" | "en attente")).cloned().collect();
            devis.sort_by(|a, b| b.depuis.cmp(&a.depuis));
            e.kpis.devis_en_attente = Some(devis.len() as u64);
            e.kpis.valeur_devis_en_attente = Some(devis.iter().filter_map(|d| d.valeur).sum());
            e.devis = Some(devis.into_iter().take(DEVIS_MAX).collect());
            let en_cours = |d: &&Devis| matches!(statut(*d).as_str(), "brouillon" | "valide" | "envoye" | "en attente");
            let mut soumissions: Vec<Devis> = tous.iter().filter(en_cours).cloned().collect();
            soumissions.sort_by(|a, b| b.modifie.cmp(&a.modifie));
            e.soumissions = Some(soumissions.into_iter().take(SOUMISSIONS_MAX).collect());
        }
        None => noter(&mut manques, "lister_devis"),
    }

    // Les pointages : cette semaine, puis la précédente — un seul qui manque, et les heures seraient fausses.
    let semaines: Option<Vec<Vec<Value>>> =
        rendus(reponses, "lister_pointages").into_iter().map(|t| t.and_then(liste)).collect();
    match semaines.filter(|s| s.len() == 2) {
        Some(s) => e.heures = Some(heures(&s.concat(), aujourdhui, &noms_des_projets)),
        None => noter(&mut manques, "lister_pointages"),
    }

    // Les factures reçues : celles des fournisseurs (`type_destinataire`), dont l'ERP garde le nom dans `client_nom`.
    // Les factures ouvertes, pour le planning : celles qui attendent leur solde, des clients et des fournisseurs (un
    // brouillon n'est pas encore envoyé, une facture annulée ne se paie plus).
    let fournisseur =
        |f: &Value| f["type_destinataire"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("fournisseur"));
    match un("lister_factures").and_then(liste) {
        Some(l) => {
            let recues = l.iter().filter(|f| fournisseur(f)).map(|f| FactureRecue {
                numero: texte(&f["numero_facture"]),
                fournisseur: texte(&f["client_nom"]),
                projet: nom_du_projet(&f["project_id"]),
                date: date(&f["date_facture"]),
                montant: montant(&f["montant_ttc"]).or_else(|| montant(&f["montant_total"])),
                solde: montant(&f["solde_du"]),
                statut: texte(&f["statut"]),
            });
            e.factures_recues = Some(recues.take(FACTURES_RECUES_MAX).collect());
            let mut ouvertes: Vec<FactureOuverte> = l
                .iter()
                .filter(|f| !matches!(normaliser(f["statut"].as_str().unwrap_or("")).as_str(), "brouillon" | "annulee"))
                .filter_map(|f| {
                    Some(FactureOuverte {
                        sorte: if fournisseur(f) { "fournisseur" } else { "client" }.into(),
                        id: entier(&f["id"]),
                        numero: texte(&f["numero_facture"]),
                        tiers: texte(&f["client_nom"]),
                        projet: nom_du_projet(&f["project_id"]),
                        statut: texte(&f["statut"]),
                        date: date(&f["date_facture"]),
                        echeance: date(&f["date_echeance"]),
                        solde: montant(&f["solde_du"]).filter(|s| *s > 0.005)?,
                    })
                })
                .collect();
            ouvertes.sort_by(|a, b| ordre_des_dates(&a.echeance, &b.echeance));
            e.factures_ouvertes = Some(ouvertes.into_iter().take(FACTURES_OUVERTES_MAX).collect());
        }
        None => noter(&mut manques, "lister_factures"),
    }

    // Les achats en route : chaque outil sa sorte, ses statuts finis écartés (reçu, annulé ; un bon d'achat commandé
    // est devenu une commande). La carte existe dès qu'un des trois outils a répondu.
    let fini = |v: &Value, finis: &[&str]| finis.contains(&normaliser(v.as_str().unwrap_or("")).as_str());
    let mut commandes = Vec::new();
    let mut achats_lus = false;
    match un("lister_bons_achat").and_then(liste) {
        Some(l) => {
            achats_lus = true;
            let mut a: Vec<Commande> = l
                .iter()
                .filter(|b| !fini(&b["statut"], &["recu", "annule", "commande"]))
                .map(|b| Commande {
                    sorte: "a_commander".into(),
                    id: entier(&b["id"]),
                    numero: texte(&b["numero_document"]),
                    fournisseur: texte(&b["fournisseur_nom"]),
                    projet: texte(&b["projet_nom"]),
                    statut: texte(&b["statut"]),
                    date: date(&b["date_echeance"]),
                    montant: positif(&b["montant_total"]),
                    texte: court(&b["notes"]),
                    ..Commande::default()
                })
                .collect();
            e.kpis.bons_a_commander = Some(a.len() as u64);
            a.sort_by(|x, y| ordre_des_dates(&x.date, &y.date));
            commandes.extend(a.into_iter().take(COMMANDES_PAR_SORTE));
        }
        None => noter(&mut manques, "lister_bons_achat"),
    }
    match un("lister_bons_commande").and_then(liste) {
        Some(l) => {
            achats_lus = true;
            let mut c: Vec<Commande> = l
                .iter()
                .filter(|b| !fini(&b["statut"], &["recu", "annule"]))
                .map(|b| Commande {
                    sorte: "commande".into(),
                    id: entier(&b["id"]),
                    numero: texte(&b["numero_bon"]).or_else(|| texte(&b["numero"])),
                    fournisseur: texte(&b["fournisseur_nom"]),
                    projet: texte(&b["projet_nom"]),
                    statut: texte(&b["statut"]),
                    depuis: date(&b["date_commande"]),
                    date: date(&b["date_livraison_prevue"]),
                    montant: positif(&b["total"]),
                    texte: court(&b["notes"]),
                    ..Commande::default()
                })
                .collect();
            e.kpis.commandes_ouvertes = Some(c.len() as u64);
            c.sort_by(|x, y| ordre_des_dates(&x.date, &y.date));
            commandes.extend(c.into_iter().take(COMMANDES_PAR_SORTE));
        }
        None => noter(&mut manques, "lister_bons_commande"),
    }
    match un("lister_livraisons").and_then(liste) {
        Some(l) => {
            achats_lus = true;
            let mut v: Vec<Commande> = l
                .iter()
                .filter(|d| date(&d["date_prevue"]).is_some_and(|j| j.as_str() >= aujourdhui))
                .map(|d| {
                    let sorte_et_zone: Vec<String> =
                        [texte(&d["type_livraison"]), texte(&d["zone_stockage"])].into_iter().flatten().collect();
                    Commande {
                        sorte: "livraison".into(),
                        id: entier(&d["id"]),
                        numero: texte(&d["reference"]),
                        projet: nom_du_projet(&d["project_id"]),
                        statut: texte(&d["statut"]),
                        date: date(&d["date_prevue"]),
                        heure: d["heure_prevue"].as_str().and_then(|h| h.get(..5)).map(str::to_string),
                        texte: (!sorte_et_zone.is_empty())
                            .then(|| sorte_et_zone.join(" · "))
                            .or_else(|| court(&d["notes"])),
                        ..Commande::default()
                    }
                })
                .collect();
            e.kpis.livraisons_prevues = Some(v.len() as u64);
            v.sort_by(|x, y| ordre_des_dates(&x.date, &y.date).then_with(|| x.heure.cmp(&y.heure)));
            commandes.extend(v.into_iter().take(COMMANDES_PAR_SORTE));
        }
        None => noter(&mut manques, "lister_livraisons"),
    }
    if achats_lus {
        e.commandes = Some(commandes);
    }

    // Les notes du calendrier, sur la fenêtre du planning (le serveur la filtre déjà : Eve la tient quand même), dans
    // l'ordre des jours ; ni le nom de qui l'a écrite.
    match un("suivi_lister_notes_calendrier").and_then(liste) {
        Some(l) => {
            let (debut, fin) = crate::planning::fenetre(aujourdhui);
            let mut notes: Vec<Note> = l
                .iter()
                .filter_map(|n| Some(Note { quand: date(&n["note_date"])?, texte: texte(&n["contenu"])? }))
                .filter(|n| n.quand >= debut && n.quand <= fin)
                .collect();
            notes.sort_by(|a, b| a.quand.cmp(&b.quand));
            notes.truncate(NOTES_MAX);
            e.agenda = Some(notes);
        }
        None => noter(&mut manques, "suivi_lister_notes_calendrier"),
    }

    match un("banque_lister_comptes").and_then(comptes) {
        Some(c) => {
            let soldes: Vec<f64> = c.iter().filter_map(|x| x.solde).collect();
            e.kpis.solde_banque = (!soldes.is_empty()).then(|| soldes.iter().sum());
            e.banque = Some(c);
        }
        None => noter(&mut manques, "banque_lister_comptes"),
    }

    let conformite = un("conformite_expirations").and_then(objet).and_then(|v| conformite(&v));
    if conformite.is_none() {
        noter(&mut manques, "conformite_expirations");
    }
    e.alertes = Some(faire_les_alertes(&e, conformite.as_ref(), aujourdhui));
    analyser_le_planning(reponses, aujourdhui, &noms_des_projets, &mut e, &mut manques);
    (e, manques)
}

/// Une vente fermée : gagnée ou perdue (avec ou sans accent, au masculin ou au féminin, comme l'ERP les écrit).
fn vente_fermee(statut: &str) -> bool {
    matches!(normaliser(statut).as_str(), "gagne" | "gagnee" | "perdu" | "perdue")
}

/// Un numéro de l'ERP, écrit en texte (les ids du Gantt sont des `VARCHAR`) ou en nombre.
fn identifiant(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| v.as_u64().map(|n| n.to_string()))
}

/// Le nom d'un employé : son prénom et son nom, rien d'autre de lui.
fn nom_d_employe(prenom: &Value, nom: &Value) -> Option<String> {
    let n = [texte(prenom), texte(nom)].into_iter().flatten().collect::<Vec<_>>().join(" ");
    (!n.is_empty()).then_some(n)
}

/// Ce que le planning prend de la lecture : les bons de travail, les ventes ouvertes, les tableaux du Kanban, et le
/// suivi de production s'il a été lu. Un outil qu'on n'a pas appelé (le suivi, hors de la demande) ne manque pas.
fn analyser_le_planning(
    reponses: &[(&str, Result<String, String>)],
    aujourdhui: &str,
    noms_des_projets: &HashMap<u64, (String, Option<String>)>,
    e: &mut Entreprise,
    manques: &mut Vec<String>,
) {
    let un = |outil: &str| rendus(reponses, outil).into_iter().next().flatten();
    let appele = |outil: &str| reponses.iter().any(|(o, _)| *o == outil);
    let nom_du_projet = |v: &Value| entier(v).and_then(|id| noms_des_projets.get(&id)).map(|(nom, _)| nom.clone());

    // Les bons de travail les plus récents, de tout statut : le Kanban de l'ERP garde une colonne aux annulés ; le
    // calendrier et le Gantt les écartent.
    if appele("lister_bons_travail") {
        match un("lister_bons_travail").and_then(liste) {
            Some(l) => {
                let bons = l.iter().filter_map(|b| {
                    Some(BonDeTravail {
                        id: entier(&b["id"])?,
                        numero: texte(&b["numero_document"]),
                        nom: texte(&b["nom"]),
                        statut: texte(&b["statut"]),
                        priorite: texte(&b["priorite"]),
                        projet_id: entier(&b["project_id"]),
                        projet: nom_du_projet(&b["project_id"]),
                        client: texte(&b["client_nom"]),
                        montant: positif(&b["montant_total"]),
                        debut: date(&b["date_debut"]),
                        fin: date(&b["date_fin"]),
                        echeance: date(&b["date_echeance"]),
                    })
                });
                e.bons_travail = Some(bons.take(BONS_TRAVAIL_MAX).collect());
            }
            None => noter(manques, "lister_bons_travail"),
        }
    }

    // Les ventes ouvertes : ni gagnées ni perdues (leurs colonnes du Kanban ne gardent que leur compte).
    if appele("lister_opportunites") {
        match un("lister_opportunites").and_then(liste) {
            Some(l) => {
                let ouvertes = l.iter().filter(|o| !vente_fermee(o["statut"].as_str().unwrap_or(""))).filter_map(|o| {
                    Some(Opportunite {
                        id: entier(&o["id"])?,
                        nom: texte(&o["nom"])?,
                        statut: texte(&o["statut"]),
                        montant: positif(&o["montant_estime"]),
                        probabilite: entier(&o["probabilite"]).filter(|p| *p <= 100),
                        debut: date(&o["date_debut_prevu"]).or_else(|| date(&o["created_at"])),
                        cloture: date(&o["date_cloture_prevue"]).or_else(|| date(&o["date_fin_prevue"])),
                        origine: texte(&o["source"]),
                    })
                });
                e.opportunites = Some(ouvertes.take(OPPORTUNITES_MAX).collect());
            }
            None => noter(manques, "lister_opportunites"),
        }
    }

    // Les tableaux du Kanban : les colonnes de chacun dans l'ordre de l'ERP et sa règle (`suivi_colonnes`, qui ne lit
    // aucune base), combien de cartes il compte (`suivi_etat_kanban`). L'un sans l'autre se garde.
    let colonnes = un("suivi_colonnes").and_then(objet);
    let comptes = un("suivi_etat_kanban").and_then(objet);
    for (outil, lu) in [("suivi_colonnes", &colonnes), ("suivi_etat_kanban", &comptes)] {
        if appele(outil) && lu.is_none() {
            noter(manques, outil);
        }
    }
    if colonnes.is_some() || comptes.is_some() {
        let mut cles: Vec<String> = Vec::new();
        for v in [&colonnes, &comptes].into_iter().flatten() {
            for cle in v.as_object().into_iter().flat_map(|o| o.keys()) {
                if !cles.contains(cle) {
                    cles.push(cle.clone());
                }
            }
        }
        let tableaux = cles.into_iter().map(|cle| {
            let c = colonnes.as_ref().map(|v| &v[cle.as_str()]);
            let n = comptes.as_ref().map(|v| &v[cle.as_str()]);
            let statuts = c.and_then(|c| c["statuts"].as_array()).map(|l| l.iter().filter_map(texte).collect());
            let par_colonne = n.and_then(|n| n["par_colonne"].as_object()).map(|o| {
                o.iter().filter_map(|(statut, nb)| Some((ecourter(statut, TEXTE_MAX), entier(nb)?))).collect()
            });
            TableauDuKanban {
                statuts: statuts.unwrap_or_default(),
                garde: c.and_then(|c| texte(&c["garde"])),
                total: n.and_then(|n| entier(&n["total"])),
                par_colonne: par_colonne.unwrap_or_default(),
                cle,
            }
        });
        e.kanban = Some(tableaux.collect());
    }

    // Le suivi de production, s'il a été lu. Qui fait une opération : son nom d'après les pointages de ces deux
    // semaines, puis d'après la liste des employés — rien d'autre d'eux ne se garde.
    let mut employes: HashMap<u64, String> = HashMap::new();
    for p in rendus(reponses, "lister_pointages").into_iter().flatten().filter_map(liste).flatten() {
        if let (Some(id), Some(nom)) = (entier(&p["employee_id"]), nom_d_employe(&p["prenom"], &p["nom_employe"])) {
            employes.insert(id, nom);
        }
    }
    if appele("lister_employes") {
        match un("lister_employes").and_then(liste) {
            Some(l) => {
                for x in &l {
                    if let (Some(id), Some(nom)) = (entier(&x["id"]), nom_d_employe(&x["prenom"], &x["nom"])) {
                        employes.insert(id, nom);
                    }
                }
            }
            None => noter(manques, "lister_employes"),
        }
    }
    if appele("suivi_lister_operations") {
        match un("suivi_lister_operations").and_then(objet).filter(|v| v["operations"].is_array()) {
            Some(v) => {
                let (debut, _) = crate::planning::fenetre(aujourdhui);
                let operations = v["operations"].as_array().into_iter().flatten().filter_map(|o| {
                    let id = entier(&o["id"])?;
                    let (d, f) = (date(&o["date_debut"]), date(&o["date_fin"]));
                    // Sans date, elle ne se place nulle part ; finie avant la fenêtre du planning, elle n'y paraît
                    // plus.
                    if f.as_ref().or(d.as_ref())? < &debut {
                        return None;
                    }
                    Some(Operation {
                        id,
                        bt_id: entier(&o["formulaire_bt_id"]),
                        bt_numero: texte(&o["bt_numero"]),
                        nom: texte(&o["nom"]).unwrap_or_else(|| format!("Opération {id}")),
                        statut: texte(&o["statut"]),
                        debut: d,
                        fin: f,
                        heures_prevues: montant(&o["heures_prevues"]).filter(|h| *h >= 0.0),
                        heures_reelles: montant(&o["heures_reelles"]).filter(|h| *h >= 0.0),
                        responsable: entier(&o["employee_id"]).and_then(|i| employes.get(&i).cloned()),
                        fournisseur: texte(&o["fournisseur"]),
                    })
                });
                e.operations = Some(operations.take(OPERATIONS_MAX).collect());
            }
            None => noter(manques, "suivi_lister_operations"),
        }
    }
    if appele("suivi_lister_dependances_gantt") {
        match un("suivi_lister_dependances_gantt").and_then(liste) {
            Some(l) => {
                let dependances = l.iter().filter_map(|d| {
                    Some(DependanceDuGantt {
                        de_sorte: texte(&d["source_type"])?,
                        de_id: identifiant(&d["source_id"])?,
                        vers_sorte: texte(&d["target_type"])?,
                        vers_id: identifiant(&d["target_id"])?,
                        sorte: texte(&d["dependency_type"]).unwrap_or_else(|| "finish_to_start".into()),
                        decalage: d["lag_days"].as_i64().unwrap_or(0),
                    })
                });
                e.dependances = Some(dependances.collect());
            }
            None => noter(manques, "suivi_lister_dependances_gantt"),
        }
    }
    if appele("suivi_lister_baselines") {
        match un("suivi_lister_baselines").and_then(liste) {
            Some(l) => {
                let references = l.iter().filter_map(|r| {
                    Some(Reference {
                        sorte: texte(&r["item_type"])?,
                        id: identifiant(&r["item_id"])?,
                        debut: date(&r["baseline_start"])?,
                        fin: date(&r["baseline_end"]),
                    })
                });
                e.references = Some(references.collect());
            }
            None => noter(manques, "suivi_lister_baselines"),
        }
    }
}

/// Les comptes bancaires du texte de `banque_lister_comptes`, le seul outil lu qui rend du texte plutôt que du JSON :
/// « #3 Compte courant (CHEQUE, GL 1010) ***1234 », puis « solde publie : 12,345.67 $   solde banque : 12,000.00 $
/// ECART : -345.67 $ » (ou « solde banque : inconnu (aucun releve importe) »), puis « 4 operation(s) en attente ». Sans
/// compte : une liste vide ; un texte d'erreur ou un compte qui ne se lit pas : rien.
fn comptes(texte: &str) -> Option<Vec<Compte>> {
    let t = texte.trim();
    if t.starts_with("Aucun compte bancaire") {
        return Some(Vec::new());
    }
    let mut lignes = t.lines().map(str::trim).filter(|l| !l.is_empty());
    if !lignes.next()?.contains("compte(s) bancaire(s)") {
        return None;
    }
    // Un montant de Python (`{:,.2f}`) : la virgule des milliers, le point décimal.
    let somme = |l: &str, motif: &str| -> Option<f64> {
        let apres = &l[l.find(motif)? + motif.len()..];
        apres.split('$').next()?.trim().replace(',', "").parse::<f64>().ok().filter(|f| f.is_finite())
    };
    let mut comptes: Vec<Compte> = Vec::new();
    for ligne in lignes {
        if let Some(reste) = ligne.strip_prefix('#') {
            // « 3 Compte courant (CHEQUE, GL 1010) ***1234 » : le nom va jusqu'à la parenthèse qui porte « GL ».
            let (_, corps) = reste.split_once(' ')?;
            let gl = corps.rfind(", GL ")?;
            let nom = corps[..gl].rfind(" (").map(|i| corps[..i].trim())?;
            comptes.push(Compte { nom: ecourter(nom, TEXTE_MAX), ..Compte::default() });
        } else {
            let c = comptes.last_mut()?;
            if ligne.starts_with("solde publie") {
                c.solde_publie = somme(ligne, "solde publie :");
                c.solde = somme(ligne, "solde banque :");
            } else if let Some(n) = ligne.strip_suffix("operation(s) en attente") {
                c.a_classer = n.trim().parse().ok()?;
            }
        }
    }
    Some(comptes)
}

/// Ce qui est à renouveler (`conformite_expirations`) : combien de licences RBQ, de cartes CCQ, d'attestations, et la
/// première date. Les cartes portent le nom de l'employé : Eve n'en garde que le nombre.
#[derive(Clone, Debug, PartialEq)]
struct Conformite {
    licences: u64,
    cartes: u64,
    attestations: u64,
    premiere: Option<String>,
}

fn conformite(v: &Value) -> Option<Conformite> {
    let (l, c, a) = (v["licences"].as_array()?, v["cartes"].as_array()?, v["attestations"].as_array()?);
    let premiere = l
        .iter()
        .chain(a)
        .filter_map(|x| date(&x["date_expiration"]))
        .chain(c.iter().filter_map(|x| date(&x["date_renouvellement"])))
        .min();
    Some(Conformite { licences: l.len() as u64, cartes: c.len() as u64, attestations: a.len() as u64, premiere })
}

fn pluriel(n: u64) -> &'static str {
    if n > 1 { "s" } else { "" }
}

/// « 9 715 $ » : au dollar, une espace fine entre les milliers.
pub fn dollars(v: f64) -> String {
    format!("{}\u{a0}$", milliers(v))
}

/// Des heures comme on les écrit : « 36,25 », « 3,5 », « 9 ».
fn heures_ecrites(h: f64) -> String {
    let t = format!("{h:.2}");
    t.trim_end_matches('0').trim_end_matches('.').replace('.', ",")
}

/// « a, b et c ».
fn en_liste(parties: &[String]) -> String {
    match parties {
        [] => String::new(),
        [seul] => seul.clone(),
        [debut @ .., dernier] => format!("{} et {dernier}", debut.join(", ")),
    }
}

/// Les alertes, de la plus grave à la moins grave : les factures en retard (hautes dès qu'une a plus de 30 jours), la
/// conformité à renouveler (haute à moins de deux semaines), le stock sous le seuil, les opérations bancaires à
/// classer.
fn faire_les_alertes(e: &Entreprise, conformite: Option<&Conformite>, aujourdhui: &str) -> Vec<Alerte> {
    let mut alertes = Vec::new();
    let alerte =
        |texte: String, gravite: &str, sorte: &str| Alerte { texte, gravite: gravite.into(), sorte: sorte.into() };
    if let Some(n) = e.kpis.factures_en_retard.filter(|n| *n > 0) {
        let vieille = e.balance_agee.as_ref().is_some_and(|b| b.de_31_a_60 + b.de_61_a_90 + b.plus_de_90 > 0.0)
            || e.factures_en_retard.iter().flatten().any(|f| f.jours.is_some_and(|j| j > 30));
        let combien = e.kpis.en_retard.map(|m| format!(" : {}", dollars(m))).unwrap_or_default();
        let texte = format!("{n} facture{} en retard{combien}", pluriel(n));
        alertes.push(alerte(texte, if vieille { "haute" } else { "moyenne" }, "factures"));
    }
    if let Some(c) = conformite.filter(|c| c.licences + c.cartes + c.attestations > 0) {
        let mut parties = Vec::new();
        for (n, quoi) in [(c.licences, "licence{} RBQ"), (c.cartes, "carte{} CCQ"), (c.attestations, "attestation{}")] {
            if n > 0 {
                parties.push(format!("{n} {}", quoi.replace("{}", pluriel(n))));
            }
        }
        let total = c.licences + c.cartes + c.attestations;
        let quand = c.premiere.as_deref().map(|d| crate::ambiance::date_longue(d).to_lowercase());
        let texte = match (&quand, total) {
            (Some(q), 1) => format!("{} à renouveler d'ici le {q}", en_liste(&parties)),
            (Some(q), _) => format!("{} à renouveler, dès le {q}", en_liste(&parties)),
            (None, _) => format!("{} à renouveler d'ici {CONFORMITE_JOURS} jours", en_liste(&parties)),
        };
        let proche =
            c.premiere.as_deref().and_then(|d| jours_entre(aujourdhui, d)).is_some_and(|j| j <= CONFORMITE_URGENTE);
        alertes.push(alerte(texte, if proche { "haute" } else { "moyenne" }, "conformite"));
    }
    if let Some(n) = e.kpis.produits_sous_seuil.filter(|n| *n > 0) {
        let texte = format!("{n} produit{} sous le seuil de réapprovisionnement", pluriel(n));
        alertes.push(alerte(texte, "moyenne", "stock"));
    }
    if let Some(m) = e.heures.as_ref().and_then(|h| h.manquantes.as_ref()).filter(|m| !m.employes.is_empty()) {
        let qui = match m.employes.len() {
            n @ 4.. => format!("{n} employés"),
            _ => en_liste(&m.employes),
        };
        let quand = crate::ambiance::date_longue(&m.jour).to_lowercase();
        alertes.push(alerte(format!("Heures manquantes le {quand} : {qui}"), "moyenne", "heures"));
    }
    let a_classer: u64 = e.banque.iter().flatten().map(|c| c.a_classer).sum();
    if a_classer > 0 {
        let s = pluriel(a_classer);
        alertes.push(alerte(format!("{a_classer} opération{s} bancaire{s} à classer"), "info", "banque"));
    }
    if let Some(h) = e.heures.as_ref().filter(|h| h.a_venir > 0.0) {
        let texte = format!("{} h saisies pour des jours à venir, à vérifier", heures_ecrites(h.a_venir));
        alertes.push(alerte(texte, "info", "heures"));
    }
    let rang = |a: &Alerte| match a.gravite.as_str() {
        "haute" => 0,
        "moyenne" => 1,
        _ => 2,
    };
    alertes.sort_by_key(rang);
    alertes
}

// ---- Le point du jour --------------------------------------------------------------------------------------------

/// L'essentiel de l'entreprise pour « le point » (la voie rapide, `Centre::point_du_jour`) : une ou deux phrases — les
/// factures en retard, puis l'alerte la plus grave d'autre chose —, d'après la dernière lecture, et son âge si elle a
/// plus d'une heure. Rien sans lecture, ni quand rien ne demande d'agir.
pub fn point_du_jour(e: &Entreprise) -> Vec<String> {
    let Some(age) = e.lu_a.as_deref().and_then(secondes_depuis).filter(|_| e.etat != NON_CONNECTE) else {
        return Vec::new();
    };
    let tete = if age > 3600 {
        format!("L'entreprise, d'après ma lecture d'{}", il_y_a(age))
    } else {
        "L'entreprise".to_string()
    };
    let mut point = Vec::new();
    if let Some(n) = e.kpis.factures_en_retard.filter(|n| *n > 0) {
        let combien = match e.kpis.en_retard {
            Some(m) if m >= 1000.0 => format!(", pour environ {}", dollars((m / 100.0).round() * 100.0)),
            Some(m) if m > 0.0 => format!(", pour {}", dollars(m)),
            _ => String::new(),
        };
        point.push(format!("{tete} : {n} facture{} en retard{combien}.", pluriel(n)));
    }
    let autre = e.alertes.iter().flatten().find(|a| a.sorte != "factures" && a.gravite != "info");
    if let Some(a) = autre {
        let debut = if point.is_empty() { format!("{tete} : ") } else { "Aussi : ".to_string() };
        point.push(format!("{debut}{}.", apres_deux_points(&a.texte)));
    }
    point
}

/// Un texte qui suit un deux-points : sa première lettre en minuscule — pas celle d'un sigle (« RBQ » reste).
fn apres_deux_points(t: &str) -> String {
    let mut c = t.chars();
    match (c.next(), c.next()) {
        (Some(p), Some(s)) if p.is_uppercase() && s.is_lowercase() => {
            p.to_lowercase().chain(t[p.len_utf8()..].chars()).collect()
        }
        _ => t.to_string(),
    }
}

/// « il y a 3 heures », « il y a 2 jours ».
fn il_y_a(secondes: i64) -> String {
    let (nombre, unite) = match secondes {
        s if s < 2 * 3600 => (s / 60, "minute"),
        s if s < 48 * 3600 => (s / 3600, "heure"),
        s => (s / 86_400, "jour"),
    };
    format!("il y a {nombre} {unite}{}", if nombre > 1 { "s" } else { "" })
}

// ---- L'heure -------------------------------------------------------------------------------------------------------

fn unix_maintenant() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// L'heure du poste en ISO 8601, avec son décalage : « 2026-10-26T13:45:00-04:00 ».
pub fn maintenant_iso() -> String {
    let local = crate::journal::date_et_heure();
    // Le décalage du poste à l'instant même, arrondi au quart d'heure (celui de tout fuseau) : la seconde qui tourne
    // entre les deux lectures de l'horloge ne compte pas.
    let decalage = crate::rappels::en_secondes(&local)
        .map_or(0, |l| ((l - unix_maintenant()) as f64 / 900.0).round() as i64 * 900);
    iso(&local, decalage)
}

/// L'heure d'une lecture de la comptabilité (`registre.json`, `lu_le`) : celle du poste AVEC son décalage, comme celle
/// de la lecture de l'entreprise — l'âge d'une lecture se compte juste au changement d'heure (la QA5, passe 2 : écrite
/// sans fuseau, « Lue à 1 h 50 » à 1 h 10, et une lecture d'un quart d'heure qui « datait » de 1 h 15).
pub fn heure_de_la_lecture() -> String {
    maintenant_iso()
}

/// « 2026-10-26 13:45:00 » et son décalage en secondes → « 2026-10-26T13:45:00-04:00 ».
fn iso(local: &str, decalage: i64) -> String {
    let (signe, d) = if decalage < 0 { ('-', -decalage) } else { ('+', decalage) };
    let (jour, heure) = (local.get(..10).unwrap_or_default(), local.get(11..19).unwrap_or_default());
    format!("{jour}T{heure}{signe}{:02}:{:02}", d / 3600, d / 60 % 60)
}

/// L'instant Unix d'une heure ISO 8601 (« 2026-10-26T13:45:00-04:00 », « …Z ») ; `None` si elle ne se lit pas.
pub fn en_unix(iso: &str) -> Option<i64> {
    let local = crate::rappels::en_secondes(&iso.get(..19)?.replace('T', " "))?;
    let decalage = match iso.get(19..)? {
        "" | "Z" => 0,
        reste => {
            let signe = match reste.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let (h, m) = reste.get(1..)?.split_once(':')?;
            signe * (h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60)
        }
    };
    Some(local - decalage)
}

/// Les secondes écoulées depuis une heure ISO 8601 (l'âge d'une lecture : `lu_a`) ; `None` si elle ne se lit pas.
pub fn secondes_depuis(iso: &str) -> Option<i64> {
    Some(unix_maintenant() - en_unix(iso)?)
}

#[cfg(test)]
mod essais {
    use super::*;

    // Les réponses du vrai serveur, de la forme exacte de `mcp_server.py` (`json.dumps(…, ensure_ascii=False,
    // indent=2, default=str)`, ses en-têtes de liste, ses textes) ; les données sont inventées.

    const KPIS: &str = r#"{
  "projets_total": 14,
  "projets_en_cours": 3,
  "projets_termines": 9,
  "clients_actifs": 27,
  "contacts_total": 61,
  "opportunites_ouvertes": 5,
  "pipeline_valeur": 612500.0,
  "devis_total": 38,
  "devis_acceptes": 15,
  "factures_total": 112,
  "factures_solde_du": 48230.55,
  "revenu_encaisse_periode": 73410.2,
  "employes_actifs": 8,
  "produits_actifs": 240,
  "stock_critique": 2,
  "valeur_stock": 18450.75,
  "fournisseurs_actifs": 17,
  "bt_total": 44,
  "devis_taux_conversion_pct": 39.5,
  "periode_jours": 30
}"#;

    const ALERTES: &str = r#"{
  "factures_en_retard": {
    "nb": 3,
    "montant": 9714.92
  },
  "produits_sous_seuil": 2
}"#;

    fn agees() -> String {
        format!(
            r#"{{
  "tranches": [
    {{
      "tranche": "a_venir",
      "nb": 6,
      "montant": 38515.63
    }},
    {{
      "tranche": "0_30",
      "nb": 1,
      "montant": 2200.0
    }},
    {{
      "tranche": "31_60",
      "nb": 2,
      "montant": 7514.92
    }}
  ],
  "total_du": 48230.55,
  "factures_en_retard": [
    {{
      "numero_facture": "F-2026-0142",
      "client_nom": "Construction Exemple inc.",
      "date_echeance": "2026-09-09",
      "solde_du": 5214.9,
      "jours_retard": 47
    }},
    {{
      "numero_facture": "F-2026-0151",
      "client_nom": null,
      "date_echeance": "2026-09-23",
      "solde_du": 2300.02,
      "jours_retard": 33
    }}
  ]
}}"#
        )
    }

    const PIPELINE: &str = r#"{
  "par_statut": [
    {
      "statut": "Proposition",
      "nb": 3,
      "valeur": 420000.0
    },
    {
      "statut": "GAGNÉ",
      "nb": 4,
      "valeur": 385000.0
    },
    {
      "statut": "Qualification",
      "nb": 2,
      "valeur": 192500.0
    },
    {
      "statut": "perdue",
      "nb": 2,
      "valeur": 96000.0
    }
  ],
  "valeur_totale": 1093500.0
}"#;

    const PROJETS: &str = r#"2 projet(s) trouve(s):
[
  {
    "id": 14,
    "nom_projet": "Agrandissement, 12 rue des Exemples",
    "numero_projet": "PROJ-2026-00014",
    "client_company_id": 7,
    "client_nom_cache": "Construction Exemple inc.",
    "statut": "En cours",
    "priorite": "Haute",
    "budget_total": 425000.0,
    "prix_estime": null,
    "date_debut_reel": "2026-09-16",
    "date_fin_reel": "2026-12-15",
    "adresse_chantier": "12 rue des Exemples",
    "ville_chantier": "Exempleville",
    "created_at": "2026-09-01T10:00:00"
  },
  {
    "id": 12,
    "nom_projet": "Toiture,\n 8 chemin du Modèle",
    "numero_projet": null,
    "client_nom_cache": null,
    "statut": "EN COURS",
    "budget_total": 0.0,
    "prix_estime": 38000.0,
    "date_debut_reel": null
  }
]"#;

    const DEVIS: &str = r#"5 devis trouve(s):
[
  {
    "id": 35,
    "numero_devis": "DEV-2026-035",
    "nom_projet": "Cuisine, 5 avenue du Modèle",
    "statut": "Brouillon",
    "investissement_total": 0.0,
    "prix_estime": 26000.0,
    "date_soumis": null,
    "created_at": "2026-10-25T14:00:00",
    "updated_at": "2026-10-26T09:00:00"
  },
  {
    "id": 33,
    "numero_devis": "DEV-2026-033",
    "nom_projet": "Terrasse, rang du Modèle",
    "statut": "En attente",
    "investissement_total": null,
    "prix_estime": 12800.0,
    "date_soumis": null,
    "created_at": "2026-10-23T09:12:44.120000",
    "updated_at": "2026-10-24T11:00:00"
  },
  {
    "id": 31,
    "numero_devis": "DEV-2026-031",
    "nom_projet": "Garage double, rue des Exemples",
    "client_nom_cache": null,
    "client_nom_direct": "Sam Exemple",
    "statut": "Envoye",
    "investissement_total": 48000.0,
    "prix_estime": 41000.0,
    "date_soumis": "2026-10-14",
    "updated_at": "2026-10-14T16:20:00"
  },
  {
    "id": 30,
    "numero_devis": "DEV-2026-030",
    "nom_projet": "Salle de bain, rue des Exemples",
    "statut": "Accepte",
    "investissement_total": 22000.0,
    "updated_at": "2026-10-12T10:00:00"
  },
  {
    "id": 28,
    "numero_devis": "DEV-2026-028",
    "nom_projet": "Véranda, rang du Modèle",
    "statut": "Refuse",
    "investissement_total": 61000.0,
    "updated_at": "2026-10-02T10:00:00"
  }
]"#;

    /// Les pointages de cette semaine (le lundi 26 octobre) : un pointage ouvert, des heures saisies d'avance.
    const POINTAGES_CETTE_SEMAINE: &str = r#"3 pointage(s) trouve(s):
[
  {
    "id": 503,
    "employee_id": 2,
    "project_id": 12,
    "formulaire_bt_id": 41,
    "operation_id": null,
    "punch_in": "2026-10-27T07:00:00",
    "punch_out": "2026-10-27T15:00:00",
    "total_hours": 8.0,
    "type_travail": "normal",
    "notes": null,
    "validated": false,
    "billable": true,
    "is_billed": false,
    "finalized_at": null,
    "prenom": "Sam",
    "nom_employe": "Exemple"
  },
  {
    "id": 502,
    "employee_id": 2,
    "project_id": 12,
    "punch_in": "2026-10-26T07:00:00",
    "punch_out": null,
    "total_hours": null,
    "prenom": "Sam",
    "nom_employe": "Exemple"
  },
  {
    "id": 501,
    "employee_id": 1,
    "project_id": 14,
    "punch_in": "2026-10-26T06:30:00",
    "punch_out": "2026-10-26T10:00:00",
    "total_hours": 3.5,
    "prenom": "Alex",
    "nom_employe": "Modèle"
  }
]"#;

    /// La semaine d'avant : Sam sans heure le vendredi ; Kim, une durée sans `total_hours`.
    fn pointages_semaine_passee() -> String {
        let mut l = Vec::new();
        for (jour, n) in [("2026-10-19", 1), ("2026-10-20", 2), ("2026-10-21", 3), ("2026-10-22", 4)] {
            l.push(json!({ "id": 400 + n, "employee_id": 2, "project_id": 14, "punch_in": format!("{jour}T07:00:00"),
                           "punch_out": format!("{jour}T15:00:00"), "total_hours": 8.0, "prenom": "Sam",
                           "nom_employe": "Exemple" }));
        }
        for (jour, n) in [("2026-10-19", 1), ("2026-10-20", 2), ("2026-10-21", 3), ("2026-10-22", 4), ("2026-10-23", 5)]
        {
            l.push(json!({ "id": 300 + n, "employee_id": 1, "project_id": 9, "punch_in": format!("{jour}T06:00:00"),
                           "punch_out": format!("{jour}T15:00:00"), "total_hours": 9.0, "prenom": "Alex",
                           "nom_employe": "Modèle" }));
        }
        l.push(json!({ "id": 299, "employee_id": 3, "project_id": null, "punch_in": "2026-10-23T07:00:00",
                       "punch_out": "2026-10-23T15:30:00", "total_hours": null, "prenom": "Kim",
                       "nom_employe": "Essai" }));
        format!("{} pointage(s) trouve(s):\n{}", l.len(), serde_json::to_string_pretty(&l).unwrap())
    }

    /// Les factures, clients et fournisseurs mêlées (`type_destinataire`, absent d'une facture d'avant sa colonne) ; un
    /// brouillon, pas encore envoyé.
    const FACTURES: &str = r#"5 facture(s) trouvée(s):
[
  {
    "id": 211,
    "numero_facture": "FACT-2026-00211",
    "client_nom": "Habitations Modèle inc.",
    "project_id": 14,
    "type_destinataire": "client",
    "date_facture": "2026-10-26",
    "date_echeance": "2026-11-25",
    "statut": "BROUILLON",
    "montant_ttc": 900.0,
    "solde_du": 900.0
  },
  {
    "id": 210,
    "numero_facture": "FACT-2026-00210",
    "client_company_id": null,
    "client_nom": "Béton Exemple ltée",
    "project_id": 14,
    "type_document": "FACTURE",
    "type_destinataire": "fournisseur",
    "date_facture": "2026-10-23",
    "date_echeance": "2026-11-22",
    "statut": "ENVOYEE",
    "montant_ht": 3000.0,
    "tps": 150.0,
    "tvq": 299.25,
    "montant_ttc": 3449.25,
    "montant_total": 3449.25,
    "montant_paye": 0.0,
    "solde_du": 3449.25,
    "conditions_paiement": "Net 30",
    "date_paiement": null,
    "mode_paiement": null
  },
  {
    "id": 209,
    "numero_facture": "FACT-2026-00209",
    "client_nom": "Construction Exemple inc.",
    "project_id": 14,
    "type_destinataire": "client",
    "date_facture": "2026-10-22",
    "montant_ttc": 25000.0,
    "solde_du": 25000.0,
    "statut": "ENVOYEE"
  },
  {
    "id": 207,
    "numero_facture": "FACT-2026-00207",
    "client_nom": "Quincaillerie Démo",
    "project_id": 99,
    "type_destinataire": "FOURNISSEUR",
    "date_facture": "2026-10-20",
    "montant_ttc": null,
    "montant_total": 412.5,
    "solde_du": 0.0,
    "statut": "PAYEE"
  },
  {
    "id": 150,
    "numero_facture": "FACT-2026-00150",
    "client_nom": "Habitations Modèle inc.",
    "type_destinataire": null,
    "date_facture": "2026-08-01",
    "statut": "PAYEE"
  }
]"#;

    const BONS_COMMANDE: &str = r#"3 bon(s) de commande trouvé(s):
[
  {
    "id": 5,
    "numero": "BC-2026-005",
    "numero_bon": null,
    "fournisseur_nom": "Bois Exemple inc.",
    "projet_nom": "Agrandissement, 12 rue des Exemples",
    "sous_total": 8000.0,
    "tps": 400.0,
    "tvq": 798.0,
    "total": 9198.0,
    "statut": "commande",
    "date_commande": "2026-10-20",
    "date_livraison_prevue": "2026-10-29",
    "notes": "Fermes de toit, livrer avant 9 h"
  },
  {
    "id": 4,
    "numero": "BC-2026-004",
    "fournisseur_nom": "Quincaillerie Démo",
    "total": 640.0,
    "statut": "recu",
    "date_commande": "2026-10-10",
    "date_livraison_prevue": "2026-10-15"
  },
  {
    "id": 3,
    "numero": "BC-2026-003",
    "numero_bon": "PO-77",
    "fournisseur_nom": "Fenêtres Modèle",
    "projet_nom": null,
    "total": 15400.0,
    "statut": "envoye",
    "date_commande": "2026-10-08",
    "date_livraison_prevue": null,
    "notes": null
  }
]"#;

    const BONS_ACHAT: &str = r#"2 bon(s) d'achat trouvé(s):
[
  {
    "id": 40,
    "numero_document": "BA-2026-040",
    "type_formulaire": "BON_ACHAT",
    "statut": "EN_ATTENTE",
    "priorite": "NORMAL",
    "montant_total": 1250.0,
    "date_creation": "2026-10-22T10:00:00",
    "date_echeance": "2026-10-28",
    "notes": "Revêtement de plancher du balcon",
    "projet_nom": "Agrandissement, 12 rue des Exemples",
    "fournisseur_nom": null
  },
  {
    "id": 38,
    "numero_document": "BA-2026-038",
    "type_formulaire": "BON_ACHAT",
    "statut": "REÇU",
    "montant_total": 300.0,
    "date_echeance": "2026-10-10"
  }
]"#;

    const LIVRAISONS: &str = r#"2 livraison(s):
[
  {
    "id": 8,
    "reference": "LIV-2026-008",
    "project_id": 14,
    "fournisseur_id": 3,
    "date_prevue": "2026-10-29",
    "heure_prevue": "08:30:00",
    "date_effective": null,
    "heure_effective": null,
    "statut": "planifiee",
    "type_livraison": "Matériaux",
    "zone_stockage": "Cour arrière",
    "notes": null,
    "created_at": "2026-10-20T10:00:00",
    "updated_at": "2026-10-20T10:00:00"
  },
  {
    "id": 6,
    "reference": "LIV-2026-006",
    "project_id": 12,
    "date_prevue": "2026-10-20",
    "statut": "planifiee"
  }
]"#;

    const NOTES: &str = r#"3 note(s) partagee(s):
[
  {
    "id": 31,
    "note_date": "2026-11-02",
    "contenu": "Livraison des fermes de toit",
    "scope": "partage",
    "created_by_name": "Sam Exemple",
    "created_at": "2026-10-20T08:00:00"
  },
  {
    "id": 29,
    "note_date": "2026-10-27",
    "contenu": "Visite de chantier, 12 rue des Exemples",
    "scope": "partage",
    "created_by_name": "Sam Exemple",
    "created_at": "2026-10-19T16:30:00"
  },
  {
    "id": 20,
    "note_date": "2026-10-20",
    "contenu": "Une note d'avant aujourd'hui",
    "scope": "partage",
    "created_by_name": null,
    "created_at": "2026-10-10T16:30:00"
  }
]"#;

    const BANQUE: &str = "2 compte(s) bancaire(s) :

#1 Compte courant (CHEQUE, GL 1010) ***4821
    solde publie : 51,240.18 $   solde banque : 52,010.43 $   ECART : 770.25 $
    7 operation(s) en attente
#2 Marge de crédit (Exemple) (CREDIT, GL 2150)
    solde publie : -12,500.00 $   solde banque : inconnu (aucun releve importe)
    0 operation(s) en attente";

    const CONFORMITE: &str = r#"{
  "date_reference": "2026-10-26",
  "jours": 60,
  "licences": [
    {
      "id": 1,
      "numero_licence": "0000-0000-01",
      "nom_entreprise": "Construction Exemple inc.",
      "categories": ["1.2"],
      "date_expiration": "2026-11-16",
      "statut": "ACTIVE"
    }
  ],
  "cartes": [],
  "attestations": [
    {
      "id": 3,
      "type": "CNESST",
      "numero": "A-0001",
      "date_expiration": "2026-12-10",
      "statut": "VALIDE"
    }
  ],
  "total": 2
}"#;

    /// Le chantier à venir (le second appel de `lister_projets`, « En attente »).
    const PROJETS_EN_ATTENTE: &str = r#"1 projet(s) trouve(s):
[
  {
    "id": 16,
    "nom_projet": "Garage détaché, 4 rang de l'Essai",
    "numero_projet": "PROJ-2026-00016",
    "client_nom_cache": "Habitations Modèle inc.",
    "statut": "En attente",
    "budget_total": 96000.0,
    "date_debut_reel": "2026-11-09",
    "date_fin_reel": "2026-12-11",
    "ville_chantier": "Démoville"
  }
]"#;

    /// Les bons de travail (`lister_bons_travail` : sa projection dynamique, et le nom du client joint) ; l'un sans nom
    /// ni chantier, un annulé.
    const BONS_TRAVAIL: &str = r#"3 bon(s) de travail trouve(s):
[
  {
    "id": 41,
    "numero_document": "BT-2026-041",
    "nom": "Charpente et toiture",
    "statut": "EN_COURS",
    "priorite": "HAUTE",
    "project_id": 14,
    "montant_total": 18500.0,
    "date_echeance": "2026-11-06",
    "date_debut": "2026-10-21",
    "date_fin": "2026-11-05",
    "date_creation": "2026-10-14T08:00:00",
    "notes": null,
    "client_company_id": 7,
    "client_contact_id": null,
    "client_nom": "Construction Exemple inc."
  },
  {
    "id": 45,
    "numero_document": "BT-2026-045",
    "nom": null,
    "statut": "BROUILLON",
    "project_id": null,
    "montant_total": 0.0,
    "date_echeance": "2026-11-15",
    "client_nom": null
  },
  {
    "id": 30,
    "numero_document": "BT-2026-030",
    "nom": "Démolition",
    "statut": "ANNULE",
    "project_id": 14,
    "date_debut": "2026-09-12",
    "date_fin": "2026-09-15"
  }
]"#;

    /// Les opportunités du CRM (`SELECT *`) : une ouverte, une gagnée, une perdue.
    const OPPORTUNITES: &str = r#"3 opportunite(s) trouvee(s):
[
  {
    "id": 12,
    "nom": "Agrandissement d'un chalet",
    "company_id": 7,
    "contact_id": null,
    "montant_estime": 180000.0,
    "devise": "CAD",
    "statut": "Proposition",
    "probabilite": 60,
    "date_cloture_prevue": "2026-11-15",
    "source": "Référence",
    "notes": "Le client veut commencer au printemps",
    "assigned_to": 2
  },
  { "id": 11, "nom": "Toiture commerciale", "statut": "Gagné", "montant_estime": 385000.0, "probabilite": 100 },
  { "id": 10, "nom": "Terrasse", "statut": "PERDU", "montant_estime": 12000.0, "probabilite": 0 }
]"#;

    /// Les colonnes du Kanban, de la forme de `suivi_colonnes` (le serveur les tient dans son code) : les ventes n'en
    /// ont pas, seulement un renvoi au CRM.
    const COLONNES: &str = r#"{
  "projets": {
    "statuts": ["En attente", "En cours", "Termine", "Annule", "Suspendu"]
  },
  "devis": {
    "statuts": ["Brouillon", "Valide", "Envoye", "En attente", "Accepte", "Refuse", "Termine", "Annule", "Expire"],
    "garde": "Statut Accepte reserve au module Soumissions (cree le projet + notification)."
  },
  "operations": {
    "statuts": ["En attente", "En cours", "Termine", "Annule"],
    "garde": "Statut gere via le Bon de travail / Gantt."
  },
  "ventes": {
    "statuts": "voir le module CRM (lister_opportunites / creer_opportunite)",
    "garde": "Statut d'opportunite gere via le CRM."
  }
}"#;

    /// Les comptes du Kanban (`suivi_etat_kanban`) : les variantes d'un statut à part, comme en base ; un tableau que
    /// l'entreprise n'a pas.
    const KANBAN: &str = r#"{
  "projets": {
    "total": 14,
    "par_colonne": { "Termine": 9, "En cours": 2, "EN_COURS": 1, "Annule": 1, "En attente": 1 }
  },
  "devis": {
    "total": 38,
    "par_colonne": { "Accepte": 15, "Refuse": 12, "Expire": 7, "Envoye": 2, "En attente": 1, "Brouillon": 1 }
  },
  "factures": { "erreur": "lecture impossible (table/colonne absente pour ce tenant)" },
  "ventes": {
    "total": 11,
    "par_colonne": { "GAGNE": 4, "Proposition": 3, "Qualification": 2, "PERDU": 2 }
  }
}"#;

    /// Le suivi de production (`suivi_lister_operations`) : une opération finie avant la fenêtre du planning, une sans
    /// date, une donnée en sous-traitance.
    const OPERATIONS: &str = r#"{
  "operations": [
    {
      "id": 380,
      "formulaire_bt_id": 38,
      "nom": "Coffrage des fondations",
      "description": null,
      "statut": "TERMINE",
      "date_debut": "2026-08-12",
      "date_fin": "2026-08-14",
      "heures_prevues": 40.0,
      "heures_reelles": 42.0,
      "quantite": 1.0,
      "employee_id": 1,
      "fournisseur": "Interne",
      "poste_travail": null,
      "sequence_number": 1,
      "bt_numero": "BT-2026-038"
    },
    {
      "id": 501,
      "formulaire_bt_id": 41,
      "nom": "Pose des fermes de toit",
      "statut": "EN_COURS",
      "date_debut": "2026-10-21",
      "date_fin": "2026-10-24",
      "heures_prevues": 24.0,
      "heures_reelles": 20.0,
      "employee_id": 1,
      "fournisseur": "Interne",
      "sequence_number": 1,
      "bt_numero": "BT-2026-041"
    },
    {
      "id": 502,
      "formulaire_bt_id": 41,
      "nom": "Bardeaux",
      "statut": "A_FAIRE",
      "date_debut": "2026-10-27",
      "date_fin": null,
      "heures_prevues": 20.0,
      "heures_reelles": null,
      "employee_id": 3,
      "fournisseur": "Toitures Exemple",
      "sequence_number": 2,
      "bt_numero": "BT-2026-041"
    },
    {
      "id": 503,
      "formulaire_bt_id": 41,
      "nom": "Solins",
      "statut": "",
      "date_debut": null,
      "date_fin": null,
      "heures_prevues": 8.0,
      "employee_id": 2,
      "sequence_number": 3,
      "bt_numero": "BT-2026-041"
    }
  ],
  "count": 4
}"#;

    /// Les employés actifs : leur courriel et leur téléphone, qu'Eve ne garde pas.
    const EMPLOYES: &str = r#"2 employe(s) trouve(s):
[
  {
    "id": 1,
    "prenom": "Alex",
    "nom": "Modèle",
    "email": "alex@exemple.ca",
    "telephone": "555-0101",
    "poste": "Charpentier",
    "departement": "Chantier"
  },
  {
    "id": 4,
    "prenom": "Lou",
    "nom": "Démo",
    "email": "lou@exemple.ca",
    "telephone": "555-0104",
    "poste": "Apprenti",
    "departement": "Chantier"
  }
]"#;

    /// Les dépendances du Gantt : un numéro en texte (la colonne est un `VARCHAR`), un en nombre.
    const DEPENDANCES: &str = r#"2 dependance(s):
[
  {
    "id": 1,
    "source_type": "op",
    "source_id": "501",
    "target_type": "op",
    "target_id": "502",
    "dependency_type": "finish_to_start",
    "lag_days": 1,
    "created_at": "2026-10-01T09:00:00"
  },
  {
    "id": 2,
    "source_type": "devis",
    "source_id": "33",
    "target_type": "project",
    "target_id": 16,
    "dependency_type": "start_to_start",
    "lag_days": 0,
    "created_at": "2026-10-02T09:00:00"
  }
]"#;

    const REFERENCES: &str = r#"1 ligne(s) de base:
[
  {
    "id": 1,
    "item_type": "project",
    "item_id": "14",
    "baseline_start": "2026-09-16",
    "baseline_end": "2026-12-01",
    "created_at": "2026-09-01T09:00:00"
  }
]"#;

    /// Toutes les réponses d'une lecture aux dix minutes, telles que le vrai serveur les rendrait, dans l'ordre des
    /// appels.
    fn reponses() -> Vec<(&'static str, Result<String, String>)> {
        vec![
            ("tableau_kpis", Ok(KPIS.into())),
            ("tableau_alertes", Ok(ALERTES.into())),
            ("tableau_factures_agees", Ok(agees())),
            ("lister_projets", Ok(PROJETS.into())),
            ("tableau_pipeline_ventes", Ok(PIPELINE.into())),
            ("lister_devis", Ok(DEVIS.into())),
            ("lister_pointages", Ok(POINTAGES_CETTE_SEMAINE.into())),
            ("lister_pointages", Ok(pointages_semaine_passee())),
            ("suivi_lister_notes_calendrier", Ok(NOTES.into())),
            ("lister_factures", Ok(FACTURES.into())),
            ("lister_bons_commande", Ok(BONS_COMMANDE.into())),
            ("lister_bons_achat", Ok(BONS_ACHAT.into())),
            ("lister_livraisons", Ok(LIVRAISONS.into())),
            ("banque_lister_comptes", Ok(BANQUE.into())),
            ("conformite_expirations", Ok(CONFORMITE.into())),
            ("lister_projets", Ok(PROJETS_EN_ATTENTE.into())),
            ("lister_bons_travail", Ok(BONS_TRAVAIL.into())),
            ("lister_opportunites", Ok(OPPORTUNITES.into())),
            ("suivi_colonnes", Ok(COLONNES.into())),
            ("suivi_etat_kanban", Ok(KANBAN.into())),
        ]
    }

    /// Les réponses du suivi de production, qui suivent les autres quand on le demande.
    fn suivi() -> Vec<(&'static str, Result<String, String>)> {
        vec![
            ("suivi_lister_operations", Ok(OPERATIONS.into())),
            ("suivi_lister_dependances_gantt", Ok(DEPENDANCES.into())),
            ("suivi_lister_baselines", Ok(REFERENCES.into())),
            ("lister_employes", Ok(EMPLOYES.into())),
        ]
    }

    /// Les réponses, celle d'un appel remplacée (`rang` : le combientième appel de cet outil).
    fn avec(
        mut r: Vec<(&'static str, Result<String, String>)>,
        outil: &str,
        rang: usize,
        texte: Result<&str, &str>,
    ) -> Vec<(&'static str, Result<String, String>)> {
        let i = r.iter().enumerate().filter(|(_, (o, _))| *o == outil).nth(rang).map(|(i, _)| i).unwrap();
        r[i].1 = texte.map(str::to_string).map_err(str::to_string);
        r
    }

    #[test]
    fn chaque_forme_du_serveur_se_lit() {
        let (e, manques) = analyser(&reponses(), "2026-10-26");
        assert!(manques.is_empty(), "{manques:?}");
        let k = &e.kpis;
        assert_eq!((k.projets_en_cours, k.opportunites_ouvertes, k.clients_actifs), (Some(3), Some(5), Some(27)));
        assert_eq!(
            (k.a_recevoir, k.pipeline, k.encaissements_30_jours),
            (Some(48230.55), Some(612500.0), Some(73410.2))
        );
        assert_eq!((k.factures_en_retard, k.en_retard, k.produits_sous_seuil), (Some(3), Some(9714.92), Some(2)));
        assert_eq!((k.employes_actifs, k.taux_conversion_devis), (Some(8), Some(39.5)));
        // Les projets : le budget canonique, sinon l'ancien (un budget à 0 manque) ; le texte d'une ligne.
        assert_eq!(k.valeur_projets_en_cours, Some(463000.0));
        let p = e.projets.as_ref().unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].client.as_deref(), Some("Construction Exemple inc."));
        assert_eq!((p[0].debut.as_deref(), p[0].fin.as_deref()), (Some("2026-09-16"), Some("2026-12-15")));
        assert_eq!((p[0].adresse.as_deref(), p[1].adresse.as_deref()), (Some("12 rue des Exemples"), None));
        assert_eq!(
            (p[1].nom.as_str(), p[1].valeur, p[1].numero.as_deref()),
            ("Toiture, 8 chemin du Modèle", Some(38000.0), None)
        );
        // La balance âgée : « 90_plus » devient « plus_de_90 » ; une tranche absente vaut 0.
        let b = e.balance_agee.as_ref().unwrap();
        assert_eq!(
            (b.a_venir, b.de_0_a_30, b.de_31_a_60, b.de_61_a_90, b.plus_de_90),
            (38515.63, 2200.0, 7514.92, 0.0, 0.0)
        );
        assert_eq!(b.total, 48230.55);
        let f = e.factures_en_retard.as_ref().unwrap();
        assert_eq!((f[0].numero.as_deref(), f[0].solde, f[0].jours), (Some("F-2026-0142"), 5214.9, Some(47)));
        assert_eq!((f[1].client.as_deref(), f[1].echeance.as_deref()), (None, Some("2026-09-23")));
        // Le pipeline : les étapes ouvertes seulement (« GAGNÉ » et « perdue » sont fermées), leur somme au KPI.
        let pl = e.pipeline.as_ref().unwrap();
        assert_eq!(pl.iter().map(|x| x.etape.as_str()).collect::<Vec<_>>(), ["Proposition", "Qualification"]);
        assert_eq!(pl.iter().map(|x| x.valeur).sum::<f64>(), 612500.0);
        // Les devis en attente (envoyés, en attente), du plus récent au plus ancien ; leur valeur, l'investissement,
        // sinon le prix. Les soumissions en cours (sans les acceptées ni les refusées), la dernière modifiée d'abord.
        assert_eq!((k.devis_en_attente, k.valeur_devis_en_attente), (Some(2), Some(60800.0)));
        let d = e.devis.as_ref().unwrap();
        assert_eq!((d[0].numero.as_deref(), d[0].depuis.as_deref()), (Some("DEV-2026-033"), Some("2026-10-23")));
        assert_eq!((d[1].client.as_deref(), d[1].valeur), (Some("Sam Exemple"), Some(48000.0)));
        let s = e.soumissions.as_ref().unwrap();
        let numeros: Vec<&str> = s.iter().filter_map(|x| x.numero.as_deref()).collect();
        assert_eq!(numeros, ["DEV-2026-035", "DEV-2026-033", "DEV-2026-031"]);
        assert_eq!(
            (s[0].statut.as_str(), s[0].valeur, s[0].modifie.as_deref()),
            ("Brouillon", Some(26000.0), Some("2026-10-26"))
        );
        // Les factures reçues : celles des fournisseurs seulement ; le projet nommé s'il est en cours.
        let f = e.factures_recues.as_ref().unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!(
            (f[0].fournisseur.as_deref(), f[0].projet.as_deref(), f[0].montant, f[0].solde, f[0].date.as_deref()),
            (
                Some("Béton Exemple ltée"),
                Some("Agrandissement, 12 rue des Exemples"),
                Some(3449.25),
                Some(3449.25),
                Some("2026-10-23")
            )
        );
        assert_eq!(
            (f[1].numero.as_deref(), f[1].projet.as_deref(), f[1].montant),
            (Some("FACT-2026-00207"), None, Some(412.5))
        );
        // Les achats en route : à commander, commandés (pas les reçus), les livraisons à venir ; leurs comptes.
        let c = e.commandes.as_ref().unwrap();
        let vu: Vec<(&str, Option<&str>, Option<&str>)> =
            c.iter().map(|x| (x.sorte.as_str(), x.numero.as_deref(), x.date.as_deref())).collect();
        assert_eq!(
            vu,
            [
                ("a_commander", Some("BA-2026-040"), Some("2026-10-28")),
                ("commande", Some("BC-2026-005"), Some("2026-10-29")),
                ("commande", Some("PO-77"), None),
                ("livraison", Some("LIV-2026-008"), Some("2026-10-29")),
            ]
        );
        assert_eq!((c[0].texte.as_deref(), c[0].montant), (Some("Revêtement de plancher du balcon"), Some(1250.0)));
        assert_eq!(
            (c[3].heure.as_deref(), c[3].projet.as_deref(), c[3].texte.as_deref()),
            (Some("08:30"), Some("Agrandissement, 12 rue des Exemples"), Some("Matériaux · Cour arrière"))
        );
        assert_eq!((k.bons_a_commander, k.commandes_ouvertes, k.livraisons_prevues), (Some(1), Some(2), Some(1)));
        // Les heures : les deux semaines, ce qui est saisi d'avance, qui travaille, qui n'a rien vendredi.
        let h = e.heures.as_ref().unwrap();
        assert_eq!((h.semaine.as_str(), h.cette_semaine, h.semaine_passee, h.a_venir), ("2026-10-26", 3.5, 85.5, 8.0));
        assert_eq!((h.employes_actifs, h.derniere_saisie.as_deref()), (3, Some("2026-10-26T07:00")));
        let m = h.manquantes.as_ref().unwrap();
        assert_eq!((m.jour.as_str(), m.employes.as_slice()), ("2026-10-23", ["Sam Exemple".to_string()].as_slice()));
        let noms: Vec<&str> = h.employes.iter().map(|x| x.nom.as_str()).collect();
        assert_eq!(noms, ["Alex Modèle", "Kim Essai", "Sam Exemple"]);
        let (alex, kim, sam) = (&h.employes[0], &h.employes[1], &h.employes[2]);
        assert_eq!((alex.cette_semaine, alex.semaine_passee, alex.jours.len(), alex.au_travail), (3.5, 45.0, 6, false));
        assert_eq!((kim.semaine_passee, kim.jours.len()), (8.5, 1), "une durée tirée du pointage");
        assert_eq!((sam.cette_semaine, sam.semaine_passee, sam.a_venir, sam.au_travail), (0.0, 32.0, 8.0, true));
        assert_eq!(h.par_projet.len(), 1);
        assert_eq!(
            (h.par_projet[0].nom.as_str(), h.par_projet[0].numero.as_deref(), h.par_projet[0].heures),
            ("Agrandissement, 12 rue des Exemples", Some("PROJ-2026-00014"), 3.5)
        );
        // L'agenda : les notes de la fenêtre du planning (du 1er septembre au 31 décembre), dans l'ordre ; ni le nom de
        // qui l'a écrite. La carte du tableau de bord n'en montre que ce qui vient.
        let a = e.agenda.as_ref().unwrap();
        assert_eq!(a.iter().map(|n| n.quand.as_str()).collect::<Vec<_>>(), ["2026-10-20", "2026-10-27", "2026-11-02"]);
        assert_eq!(a[1].texte, "Visite de chantier, 12 rue des Exemples");
        // La banque : le nom jusqu'à la parenthèse du grand livre ; un solde de banque inconnu manque.
        let c = e.banque.as_ref().unwrap();
        assert_eq!(
            (c[0].nom.as_str(), c[0].solde, c[0].solde_publie, c[0].a_classer),
            ("Compte courant", Some(52010.43), Some(51240.18), 7)
        );
        assert_eq!(
            (c[1].nom.as_str(), c[1].solde, c[1].solde_publie),
            ("Marge de crédit (Exemple)", None, Some(-12500.0))
        );
        assert_eq!(k.solde_banque, Some(52010.43));
    }

    #[test]
    fn les_alertes_vont_de_la_plus_grave_a_la_moins_grave() {
        let (e, _) = analyser(&reponses(), "2026-10-26");
        let a = e.alertes.unwrap();
        let vu: Vec<(&str, &str)> = a.iter().map(|x| (x.sorte.as_str(), x.gravite.as_str())).collect();
        assert_eq!(
            vu,
            [
                ("factures", "haute"),
                ("conformite", "moyenne"),
                ("stock", "moyenne"),
                ("heures", "moyenne"),
                ("banque", "info"),
                ("heures", "info"),
            ]
        );
        assert_eq!(a[0].texte, "3 factures en retard : 9\u{202f}715\u{a0}$");
        assert_eq!(a[1].texte, "1 licence RBQ et 1 attestation à renouveler, dès le lundi 16 novembre");
        assert_eq!(a[2].texte, "2 produits sous le seuil de réapprovisionnement");
        assert_eq!(a[3].texte, "Heures manquantes le vendredi 23 octobre : Sam Exemple");
        assert_eq!(a[4].texte, "7 opérations bancaires à classer");
        assert_eq!(a[5].texte, "8 h saisies pour des jours à venir, à vérifier");
        // À moins de deux semaines, la conformité est haute ; des factures toutes récentes, moyennes.
        let (e, _) = analyser(&reponses(), "2026-11-05");
        assert_eq!(e.alertes.as_ref().unwrap()[1].gravite, "haute");
        let mut recentes = Entreprise::default();
        recentes.kpis.factures_en_retard = Some(1);
        let a = faire_les_alertes(&recentes, None, "2026-10-26");
        assert_eq!((a[0].texte.as_str(), a[0].gravite.as_str()), ("1 facture en retard", "moyenne"));
    }

    /// Une lecture réussie, sans temps.
    fn lecture_de(entreprise: Entreprise, manques: Vec<String>, suivi: bool) -> Lecture {
        let outils = outils_de(&appels("2026-10-26", suivi)).len();
        Lecture { entreprise, manques, ouverture: Duration::ZERO, duree: Duration::ZERO, outils, suivi }
    }

    #[test]
    fn le_planning_se_lit_dans_les_formes_du_serveur() {
        let (e, manques) = analyser(&reponses(), "2026-10-26");
        assert!(manques.is_empty(), "{manques:?}");
        // Les chantiers à venir : le second appel des projets ; leur numéro dans l'ERP, comme ceux en cours.
        let a = e.projets_en_attente.as_ref().unwrap();
        assert_eq!((a.len(), a[0].id, a[0].debut.as_deref()), (1, Some(16), Some("2026-11-09")));
        assert_eq!(e.projets.as_ref().unwrap()[0].id, Some(14));
        assert_eq!(e.kpis.valeur_projets_en_cours, Some(463000.0), "les chantiers à venir n'y comptent pas");
        assert_eq!(e.devis.as_ref().unwrap()[0].id, Some(33));
        // Les bons de travail, de tout statut ; leur chantier nommé s'il est en cours ou à venir ; un montant à 0
        // manque.
        let b = e.bons_travail.as_ref().unwrap();
        assert_eq!(b.iter().map(|x| x.id).collect::<Vec<_>>(), [41, 45, 30]);
        assert_eq!(
            (b[0].projet.as_deref(), b[0].projet_id, b[0].debut.as_deref(), b[0].echeance.as_deref()),
            (Some("Agrandissement, 12 rue des Exemples"), Some(14), Some("2026-10-21"), Some("2026-11-06"))
        );
        assert_eq!(
            (b[1].nom.as_deref(), b[1].montant, b[1].projet_id, b[1].client.as_deref()),
            (None, None, None, None)
        );
        assert_eq!(b[2].statut.as_deref(), Some("ANNULE"));
        // Les ventes ouvertes seulement (« Gagné » et « PERDU » sont fermées) ; ni leurs notes.
        let o = e.opportunites.as_ref().unwrap();
        assert_eq!(o.len(), 1);
        assert_eq!(
            (o[0].id, o[0].montant, o[0].probabilite, o[0].cloture.as_deref(), o[0].origine.as_deref()),
            (12, Some(180000.0), Some(60), Some("2026-11-15"), Some("Référence"))
        );
        // Les factures ouvertes : celles qui attendent leur solde, des deux côtés (ni brouillon ni payée), la plus
        // proche échéance d'abord, sans échéance ensuite.
        let f = e.factures_ouvertes.as_ref().unwrap();
        let vu: Vec<(Option<u64>, &str, Option<&str>, f64)> =
            f.iter().map(|x| (x.id, x.sorte.as_str(), x.echeance.as_deref(), x.solde)).collect();
        assert_eq!(vu, [(Some(210), "fournisseur", Some("2026-11-22"), 3449.25), (Some(209), "client", None, 25000.0)]);
        assert_eq!(f[0].projet.as_deref(), Some("Agrandissement, 12 rue des Exemples"));
        // Les commandes gardent leur numéro dans l'ERP.
        assert_eq!(
            e.commandes.as_ref().unwrap().iter().map(|c| c.id).collect::<Vec<_>>(),
            [Some(40), Some(5), Some(3), Some(8)]
        );
        // Le Kanban : les tableaux des deux outils réunis ; les colonnes de l'ERP dans son ordre, ses comptes tels que
        // la base les écrit ; un tableau illisible n'a ni compte ni colonne ; les ventes, une règle sans colonnes.
        let k = e.kanban.as_ref().unwrap();
        let cles: Vec<&str> = k.iter().map(|t| t.cle.as_str()).collect();
        assert_eq!(cles, ["devis", "operations", "projets", "ventes", "factures"]);
        let tableau = |cle: &str| k.iter().find(|t| t.cle == cle).unwrap();
        let p = tableau("projets");
        assert_eq!(p.statuts, ["En attente", "En cours", "Termine", "Annule", "Suspendu"]);
        assert_eq!(
            (p.total, p.par_colonne.get("EN_COURS"), p.par_colonne.len(), p.garde.as_deref()),
            (Some(14), Some(&1), 5, None)
        );
        let v = tableau("ventes");
        assert!(v.statuts.is_empty(), "l'ERP ne donne pas de colonnes aux ventes");
        assert_eq!((v.garde.as_deref(), v.total), (Some("Statut d'opportunite gere via le CRM."), Some(11)));
        let fa = tableau("factures");
        assert_eq!((fa.total, fa.par_colonne.len(), fa.statuts.len()), (None, 0, 0));
        assert_eq!((tableau("operations").total, tableau("operations").statuts.len()), (None, 4));
        // Le suivi de production ne s'est pas lu : rien de lui, et rien ne manque.
        assert!(e.operations.is_none() && e.dependances.is_none() && e.references.is_none());
    }

    #[test]
    fn le_suivi_de_production_se_lit_a_la_demande() {
        let mut r = reponses();
        r.extend(suivi());
        let (e, manques) = analyser(&r, "2026-10-26");
        assert!(manques.is_empty(), "{manques:?}");
        // Les opérations : celle finie avant la fenêtre du planning et celle sans date n'y sont pas ; qui les fait,
        // nommé d'après la liste des employés, sinon d'après les pointages (Kim n'est que dans ceux-ci).
        let o = e.operations.as_ref().unwrap();
        assert_eq!(o.iter().map(|x| x.id).collect::<Vec<_>>(), [501, 502]);
        assert_eq!(
            (
                o[0].bt_id,
                o[0].bt_numero.as_deref(),
                o[0].heures_prevues,
                o[0].heures_reelles,
                o[0].responsable.as_deref()
            ),
            (Some(41), Some("BT-2026-041"), Some(24.0), Some(20.0), Some("Alex Modèle"))
        );
        assert_eq!(
            (o[1].responsable.as_deref(), o[1].fournisseur.as_deref(), o[1].fin.as_deref(), o[1].heures_reelles),
            (Some("Kim Essai"), Some("Toitures Exemple"), None, None)
        );
        // Les dépendances, telles que l'ERP les dit (un numéro en nombre se lit) ; les références.
        let d = e.dependances.as_ref().unwrap();
        assert_eq!(
            (d[0].de_sorte.as_str(), d[0].de_id.as_str(), d[0].sorte.as_str(), d[0].decalage),
            ("op", "501", "finish_to_start", 1)
        );
        assert_eq!((d[1].vers_sorte.as_str(), d[1].vers_id.as_str()), ("project", "16"));
        let x = &e.references.as_ref().unwrap()[0];
        assert_eq!(
            (x.sorte.as_str(), x.id.as_str(), x.debut.as_str(), x.fin.as_deref()),
            ("project", "14", "2026-09-16", Some("2026-12-01"))
        );
        // D'un employé, son nom seulement : ni son courriel ni son téléphone n'arrivent au fichier.
        let texte = serde_json::to_string(&e).unwrap();
        assert!(!texte.contains("@exemple.ca") && !texte.contains("555-01"), "{texte}");
        // Un outil du suivi qui flanche manque, seul ; la liste des employés aussi (les pointages nomment encore).
        let r = avec(r, "suivi_lister_operations", 0, Ok("Erreur lors de la lecture des operations."));
        let (e, manques) = analyser(&r, "2026-10-26");
        assert_eq!(manques, ["suivi_lister_operations"]);
        assert!(e.operations.is_none() && e.dependances.is_some() && e.references.is_some());
        let mut r = reponses();
        r.extend(suivi());
        let r = avec(r, "lister_employes", 0, Err("lister_employes : Unknown tool: lister_employes"));
        let (e, manques) = analyser(&r, "2026-10-26");
        assert_eq!(manques, ["lister_employes"]);
        assert_eq!(e.operations.as_ref().unwrap()[0].responsable.as_deref(), Some("Alex Modèle"));
    }

    #[test]
    fn une_lecture_sans_le_suivi_garde_celui_d_avant() {
        let mut r = reponses();
        r.extend(suivi());
        let (lu, manques) = analyser(&r, "2026-10-26");
        let avec_le_suivi = lecture_de(lu, manques, true);
        let e = Entreprise::default().suite(&avec_le_suivi, "2026-10-26T13:45:00-04:00");
        assert_eq!(e.suivi_lu_a.as_deref(), Some("2026-10-26T13:45:00-04:00"));
        assert_eq!(e.operations.as_ref().map(Vec::len), Some(2));
        // Dix minutes plus tard, une lecture sans lui : le suivi d'avant reste, avec son heure.
        let (lu, manques) = analyser(&reponses(), "2026-10-26");
        let suite = e.suite(&lecture_de(lu, manques, false), "2026-10-26T13:55:00-04:00");
        assert_eq!(
            (suite.lu_a.as_deref(), suite.suivi_lu_a.as_deref()),
            (Some("2026-10-26T13:55:00-04:00"), Some("2026-10-26T13:45:00-04:00"))
        );
        assert_eq!(
            (&suite.operations, &suite.dependances, &suite.references),
            (&e.operations, &e.dependances, &e.references)
        );
        // Le journal : combien d'outils cette lecture a appelés ; le suivi en nombres, quand il en est.
        assert!(avec_le_suivi.resume().starts_with("22 outils lus sur 22 en "), "{}", avec_le_suivi.resume());
        assert!(
            avec_le_suivi.resume().contains("le suivi de production : 2 opérations, 2 dépendances, 1 références"),
            "{}",
            avec_le_suivi.resume()
        );
        assert!(!lecture_de(Entreprise::default(), Vec::new(), false).resume().contains("suivi"));
        // La demi-heure : jamais lu, il est à relire ; lu à l'instant, non ; il y a trente et une minutes, oui ; à une
        // heure de l'avenir (l'horloge du poste a reculé), oui.
        assert!(suivi_a_relire(&Entreprise::default()));
        let lu_a = |decalage: i64| {
            let t = unix_maintenant() + decalage;
            let (jour, s) = (t.div_euclid(86_400), t.rem_euclid(86_400));
            Some(iso(&format!("{} {:02}:{:02}:{:02}", date_du_numero(jour), s / 3600, s / 60 % 60, s % 60), 0))
        };
        assert!(!suivi_a_relire(&Entreprise { suivi_lu_a: lu_a(-60), ..Entreprise::default() }));
        assert!(suivi_a_relire(&Entreprise { suivi_lu_a: lu_a(-31 * 60), ..Entreprise::default() }));
        assert!(suivi_a_relire(&Entreprise { suivi_lu_a: lu_a(3600), ..Entreprise::default() }));
    }

    #[test]
    fn un_outil_qui_flanche_manque_sans_emporter_les_autres() {
        let r = avec(reponses(), "tableau_kpis", 0, Ok("Erreur lors de la lecture des KPIs (voir logs serveur)."));
        let r = avec(r, "lister_projets", 0, Err("lister_projets : Unknown tool: lister_projets"));
        let r = avec(r, "lister_pointages", 1, Ok("Erreur lors de la lecture des pointages (voir logs serveur)."));
        let r = avec(r, "lister_bons_commande", 0, Ok("Erreur: column \"numero_bon\" does not exist"));
        let r = avec(r, "banque_lister_comptes", 0, Ok("Erreur lors de la lecture des comptes : relation absente"));
        let (e, manques) = analyser(&r, "2026-10-26");
        assert_eq!(
            manques,
            ["tableau_kpis", "lister_projets", "lister_pointages", "lister_bons_commande", "banque_lister_comptes"]
        );
        assert_eq!(e.kpis.projets_en_cours, None);
        assert_eq!(e.kpis.factures_en_retard, Some(3), "les alertes du serveur, elles, se lisent");
        assert!(e.projets.is_none() && e.banque.is_none());
        assert!(e.heures.is_none(), "une semaine sur deux : pas d'heures fausses");
        // Les achats : la carte existe sans les bons de commande, et leur compte manque.
        let sortes: Vec<&str> = e.commandes.iter().flatten().map(|c| c.sorte.as_str()).collect();
        assert_eq!((sortes, e.kpis.commandes_ouvertes), (vec!["a_commander", "livraison"], None));
        // Sans compte bancaire ni bon de commande : des listes vides, pas des manques.
        let r = avec(r, "banque_lister_comptes", 0, Ok("Aucun compte bancaire lie. Utilisez `banque_creer_compte`."));
        let vide = "0 bon(s) de commande trouvé(s):\nAucun(e) bon de commande trouvé(e).";
        let r = avec(r, "lister_bons_commande", 0, Ok(vide));
        let (e, manques) = analyser(&r, "2026-10-26");
        assert!(!manques.iter().any(|m| m == "banque_lister_comptes" || m == "lister_bons_commande"), "{manques:?}");
        assert_eq!((e.banque, e.kpis.solde_banque, e.kpis.commandes_ouvertes), (Some(Vec::new()), None, Some(0)));
    }

    #[test]
    fn les_petits_mots_du_tableau() {
        assert_eq!(
            (heures_ecrites(36.25), heures_ecrites(3.5), heures_ecrites(9.0)),
            ("36,25".into(), "3,5".into(), "9".into())
        );
        assert_eq!(apres_deux_points("Heures manquantes"), "heures manquantes");
        assert_eq!(apres_deux_points("RBQ à renouveler"), "RBQ à renouveler");
        assert_eq!(apres_deux_points("3 factures"), "3 factures");
        assert_eq!(apres_deux_points("Éléments"), "éléments");
        assert_eq!(duree_en_heures("2026-10-23T07:00:00", "2026-10-23T15:30:00"), Some(8.5));
        assert_eq!(duree_en_heures("2026-10-23T15:30:00", "2026-10-23T07:00:00"), None);
        let mut l = vec![Some("2026-10-29".to_string()), None, Some("2026-10-28".to_string())];
        l.sort_by(ordre_des_dates);
        assert_eq!(l, [Some("2026-10-28".to_string()), Some("2026-10-29".to_string()), None]);
    }

    #[test]
    fn le_json_d_un_outil_se_trouve_apres_son_en_tete() {
        assert!(objet(KPIS).is_some());
        assert_eq!(liste("0 note(s) partagee(s):\n[]"), Some(Vec::new()));
        assert_eq!(liste(PROJETS).map(|l| l.len()), Some(2));
        assert!(json_de("Erreur lors de la lecture des projets (voir logs serveur).").is_none());
        assert!(json_de("Erreur: date invalide (format AAAA-MM-JJ attendu).").is_none());
        assert!(objet(PROJETS).is_none(), "une liste n'est pas un objet");
        assert_eq!(entier(&json!(3.0)), Some(3));
        assert_eq!(entier(&json!(-1)), None);
        assert_eq!(montant(&json!("1234.50")), Some(1234.5));
        assert_eq!(date(&json!("2026-10-23T09:12:44")), Some("2026-10-23".into()));
        assert_eq!(date(&json!("hier")), None);
    }

    /// Une réponse en JSON, ou en SSE comme la donne sse-starlette (des `\r\n`, un `: ping`, une notification avant la
    /// réponse, un lot), ou sans ligne vide finale : le message de la requête se trouve.
    #[test]
    fn une_reponse_se_lit_en_json_comme_en_sse() {
        let r = br#"{"jsonrpc":"2.0","id":3,"result":{"ok":true}}"#;
        assert_eq!(message_de_la_reponse(Some("application/json"), r, 3).unwrap()["result"]["ok"], true);
        assert!(message_de_la_reponse(Some("application/json"), r, 4).is_err(), "pas la réponse à la 4");
        let sse = "event: message\r\n\
                   data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{}}\r\n\r\n\
                   : ping - 2026-10-26 13:45:00.000000+00:00\r\n\r\n\
                   event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":7,\r\ndata: \"result\":{\"n\":1}}\r\n\r\n";
        let m = message_de_la_reponse(Some("text/event-stream; charset=utf-8"), sse.as_bytes(), 7).unwrap();
        assert_eq!(m["result"]["n"], 1);
        let sans_fin =
            "data: [{\"jsonrpc\":\"2.0\",\"id\":2,\"error\":{\"code\":-32602,\"message\":\"Invalid params\"}}]";
        let m = message_de_la_reponse(Some("text/event-stream"), sans_fin.as_bytes(), 2).unwrap();
        assert_eq!(erreur_jsonrpc(&m["error"]), "Invalid params (-32602)");
        assert!(message_de_la_reponse(Some("text/html"), b"<html>Render</html>", 1).is_err());
        assert_eq!(evenements_sse("data: a\rdata: b\r\rdata:c\n"), ["a\nb", "c"]);
    }

    #[test]
    fn le_texte_d_un_resultat_reunit_ses_morceaux() {
        let morceaux =
            json!([{ "type": "text", "text": "un" }, { "type": "image" }, { "type": "text", "text": "deux" }]);
        let r = json!({ "content": morceaux, "structuredContent": { "result": "un\ndeux" } });
        assert_eq!(texte_du_resultat(&r), "un\ndeux");
    }

    /// Seules les lectures de la liste partent : un outil d'écriture est refusé avant tout envoi (un client vers un
    /// port fermé le prouve : la panne serait une panne de serveur s'il avait essayé).
    #[test]
    fn un_outil_d_ecriture_ne_part_jamais() {
        for o in OUTILS_LUS {
            assert!(permis(o), "{o}");
        }
        for (o, _) in appels("2026-10-26", true) {
            assert!(OUTILS_LUS.contains(&o), "{o}");
        }
        // Le suivi de production ne part qu'à la demande ; chaque outil de la liste sert à l'une ou l'autre lecture.
        let sans_suivi = outils_de(&appels("2026-10-26", false));
        let avec_suivi = outils_de(&appels("2026-10-26", true));
        assert_eq!((sans_suivi.len(), avec_suivi.len(), OUTILS_LUS.len()), (18, 22, 24));
        for o in OUTILS_DU_SUIVI {
            assert!(!sans_suivi.contains(&o) && avec_suivi.contains(&o), "{o}");
        }
        // La comptabilité ne part avec aucune lecture de l'entreprise : à la demande de son onglet seulement
        // (`lire_la_compta`). Les deux lectures, ensemble, prennent toute la liste, chaque outil une fois.
        assert_eq!(OUTILS_DE_LA_COMPTA, ["comptabilite_lister_registre", "comptabilite_lister_historique"]);
        for o in OUTILS_DE_LA_COMPTA {
            assert!(permis(o) && est_lecture_constructo(o) && !avec_suivi.contains(&o), "{o}");
        }
        let mut toutes: Vec<&str> = avec_suivi.iter().chain(&OUTILS_DE_LA_COMPTA).copied().collect();
        let mut liste = OUTILS_LUS.to_vec();
        toutes.sort_unstable();
        liste.sort_unstable();
        assert_eq!(toutes, liste);
        // Les colonnes du Kanban : une lecture que la garde tient pour telle, en plus du serveur.
        assert!(permis("suivi_colonnes"));
        for o in [
            "creer_facture", "supprimer_projet", "enregistrer_paiement", "modifier_projet",
            "suivi_creer_note_calendrier", "banque_creer_compte", "banque_publier_operation", "sql_admin",
            "recherche_sql", "email_envoyer",
        ] {
            assert!(!permis(o), "{o}");
        }
        let mut c = Client {
            url: "http://127.0.0.1:9/mcp".into(),
            jeton: "jeton".into(),
            session: None,
            version: None,
            suivant: 1,
            echeance: None,
        };
        assert_eq!(c.appeler("creer_facture", json!({})), Err(Panne::Refuse("creer_facture".into())));
        assert!(matches!(c.appeler("tableau_kpis", json!({})), Err(Panne::Serveur(_))), "une lecture, elle, part");
    }

    #[test]
    fn la_redirection_vers_la_barre_finale_seulement() {
        let url = "https://constructo-mcp.onrender.com/mcp";
        let voulu = Some("https://constructo-mcp.onrender.com/mcp/".to_string());
        assert_eq!(barre_finale(url, "http://constructo-mcp.onrender.com/mcp/"), voulu);
        assert_eq!(barre_finale(url, "https://Constructo-MCP.onrender.com:443/mcp/"), voulu);
        assert_eq!(barre_finale(url, "/mcp/"), voulu);
        assert_eq!(barre_finale(url, "https://ailleurs.exemple/mcp/"), None);
        assert_eq!(barre_finale(url, "https://constructo-mcp.onrender.com/autre/"), None);
        assert_eq!(barre_finale(url, "/mcp"), None);
        assert_eq!(
            barre_finale("http://127.0.0.1:47997/mcp", "http://127.0.0.1:47997/mcp/").as_deref(),
            Some("http://127.0.0.1:47997/mcp/")
        );
    }

    #[test]
    fn l_heure_iso_fait_l_aller_retour() {
        assert_eq!(iso("2026-10-26 13:45:00", -4 * 3600), "2026-10-26T13:45:00-04:00");
        assert_eq!(iso("2026-10-26 13:45:00", 5 * 3600 + 1800), "2026-10-26T13:45:00+05:30");
        let utc = numero_du_jour("2026-10-26").unwrap() * 86_400 + 17 * 3600 + 45 * 60;
        assert_eq!(en_unix("2026-10-26T13:45:00-04:00"), Some(utc));
        assert_eq!(en_unix("2026-10-26T17:45:00Z"), Some(utc));
        assert_eq!(en_unix("2026-10-26T19:15:00+01:30"), Some(utc));
        assert_eq!(en_unix("le 26 octobre"), None);
        assert_eq!(en_unix("2026-10-26T13:45:00 04:00"), None);
        let age = secondes_depuis(&maintenant_iso()).unwrap();
        assert!((0..5).contains(&age), "{age}");
        assert_eq!(il_y_a(3 * 3600 + 10), "il y a 3 heures");
        assert_eq!(il_y_a(65), "il y a 1 minute");
        assert_eq!(il_y_a(3 * 86_400), "il y a 3 jours");
    }

    #[test]
    fn le_point_dit_l_essentiel_en_deux_phrases() {
        let (lu, _) = analyser(&reponses(), "2026-10-26");
        let lecture = lecture_de(lu, Vec::new(), false);
        let e = Entreprise::lue(&lecture, &maintenant_iso());
        assert_eq!(
            point_du_jour(&e),
            [
                "L'entreprise : 3 factures en retard, pour environ 9\u{202f}700\u{a0}$.",
                "Aussi : 1 licence RBQ et 1 attestation à renouveler, dès le lundi 16 novembre."
            ]
        );
        // Une lecture vieille de trois heures le dit ; sans facture en retard, l'alerte ouvre la phrase.
        let mut vieille = e.manquee("Constructo AI ne répond pas", &maintenant_iso());
        let t = unix_maintenant() - 3 * 3600 - 60;
        let (jour, s) = (t.div_euclid(86_400), t.rem_euclid(86_400));
        vieille.lu_a =
            Some(iso(&format!("{} {:02}:{:02}:{:02}", date_du_numero(jour), s / 3600, s / 60 % 60, s % 60), 0));
        vieille.kpis.factures_en_retard = Some(0);
        let p = point_du_jour(&vieille);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(p[0].starts_with("L'entreprise, d'après ma lecture d'il y a 3 heures : 1 licence RBQ"), "{p:?}");
        // Rien sans lecture, ni déconnectée, ni quand rien ne demande d'agir.
        assert!(point_du_jour(&Entreprise::default()).is_empty());
        assert!(point_du_jour(&Entreprise { etat: NON_CONNECTE.into(), ..e.clone() }).is_empty());
        let calme = Entreprise { alertes: Some(Vec::new()), kpis: Kpis::default(), ..e };
        assert!(point_du_jour(&calme).is_empty());
    }

    /// Le fichier : d'un geste, relu tel quel ; un champ absent n'y est pas écrit (aucun `null`) ; déconnectée, rien de
    /// l'entreprise.
    #[test]
    fn le_fichier_garde_la_lecture_et_ne_dit_que_ce_qu_il_sait() {
        let dossier = std::env::temp_dir().join(format!("eve-entreprise-{}", std::process::id()));
        fs::create_dir_all(&dossier).unwrap();
        let (lu, manques) = analyser(&reponses(), "2026-10-26");
        let e = Entreprise::lue(&lecture_de(lu, manques, false), "2026-10-26T13:45:00-04:00");
        ecrire(&dossier, &e).unwrap();
        assert_eq!(charger(&dossier), e);
        let texte = fs::read_to_string(dossier.join(FICHIER)).unwrap();
        assert!(!texte.contains("null") && !texte.contains("\"permis\"") && !texte.contains("\"erreur\""), "{texte}");
        for cle in [
            "\"lu_a\"", "\"etat\": \"a_jour\"", "\"kpis\"", "\"0_30\"", "\"plus_de_90\"", "\"pipeline\"", "\"agenda\"",
            "\"soumissions\"", "\"factures_recues\"", "\"commandes\"", "\"heures\"", "\"par_projet\"",
        ] {
            assert!(texte.contains(cle), "{cle} manque");
        }
        // Une lecture manquée garde la dernière, avec son heure ; une nouvelle commence sans l'erreur d'avant.
        let m = e.manquee("Constructo AI ne répond pas", "2026-10-26T13:55:00-04:00");
        assert_eq!(
            (m.etat.as_str(), m.lu_a.as_deref(), m.projets.as_ref().map(Vec::len)),
            (ERREUR, Some("2026-10-26T13:45:00-04:00"), Some(2))
        );
        assert_eq!(m.en_cours().erreur, None);
        let n = Entreprise::non_connecte(true);
        ecrire(&dossier, &n).unwrap();
        assert_eq!(fs::read_to_string(dossier.join(FICHIER)).unwrap(), "{\n  \"etat\": \"non_connecte\"\n}");
        let refuse = Entreprise::non_connecte(false);
        assert_eq!((refuse.permis, refuse.erreur.is_some()), (Some(false), true));
        fs::write(dossier.join(FICHIER), b"illisible").unwrap();
        assert_eq!(charger(&dossier), Entreprise::default());
        let _ = fs::remove_dir_all(&dossier);
    }

    /// Un jeton d'accès du faux Constructo AI d'`eve --essai-constructo`, monté ici : la connexion jouée comme dans
    /// `connexion.rs` (l'enregistrement, la page de connexion, l'échange du code).
    fn jeton_du_faux(faux: &crate::essais::FauxConstructo, nom: &str) -> (String, crate::connexion::Lien) {
        use crate::connexion::{Etat, Lien, Retour};
        let dossier = std::env::temp_dir().join(format!("eve-entreprise-{}-{nom}", std::process::id()));
        fs::create_dir_all(&dossier).unwrap();
        let lien = Lien { serveur: format!("http://127.0.0.1:{}", faux.port), fichier: dossier.join("connexion.bin") };
        let (etat, en_cours, url) = lien.preparer(&Etat::default(), 47821).unwrap();
        let ou = crate::essais::demander_sans_suivre(&url).unwrap().1.unwrap();
        let (_, requete) = ou.split_once('?').unwrap();
        let param = |nom: &str| {
            let valeur = requete.split('&').filter_map(|p| p.split_once('=')).find(|(n, _)| *n == nom).map(|(_, v)| v);
            crate::ecran::decoder(valeur.unwrap_or_default())
        };
        let retour =
            Retour { code: param("code"), etat_oauth: param("state"), emetteur: param("iss"), ..Retour::default() };
        let code = lien.verifier_le_retour(Some(&en_cours), &retour).unwrap();
        (lien.echanger(&etat, &en_cours, &code).unwrap().acces, lien)
    }

    /// Une lecture de bout en bout sur le faux Constructo AI (jamais le vrai) : la session — `initialize`, la
    /// notification, un appel par outil, le DELETE —, les réponses en SSE, l'entreprise inventée lue en entier ; puis
    /// un outil en panne, qui manque sans emporter les autres ; la redirection de Starlette, prise une fois ; un jeton
    /// refusé ; un serveur absent.
    #[test]
    fn une_lecture_de_bout_en_bout_sur_le_faux_serveur() {
        let faux = crate::essais::FauxConstructo::demarrer(0, 3600, 0).unwrap();
        let (jeton, lien) = jeton_du_faux(&faux, "lecture");
        let (ressource, aujourdhui) = (lien.ressource(), crate::journal::date());
        let l = lire(&ressource, &jeton, &aujourdhui, false).unwrap();
        assert!(l.manques.is_empty(), "{:?}", l.manques);
        let e = &l.entreprise;
        let k = &e.kpis;
        assert_eq!((k.projets_en_cours, k.factures_en_retard, k.devis_en_attente), (Some(3), Some(3), Some(3)));
        assert_eq!((k.valeur_projets_en_cours, k.valeur_devis_en_attente), (Some(535500.0), Some(92300.0)));
        assert_eq!(e.projets.as_ref().map(Vec::len), Some(3), "le projet terminé n'y est pas");
        assert_eq!(e.factures_en_retard.as_ref().map(Vec::len), Some(3));
        assert_eq!(e.soumissions.as_ref().map(Vec::len), Some(4), "le devis accepté n'y est pas");
        assert_eq!(e.factures_recues.as_ref().map(Vec::len), Some(2), "celles des fournisseurs");
        let sortes: Vec<&str> = e.commandes.iter().flatten().map(|c| c.sorte.as_str()).collect();
        assert_eq!(sortes, ["a_commander", "commande", "livraison"], "la commande reçue n'y est pas");
        let h = e.heures.as_ref().unwrap();
        assert_eq!((h.cette_semaine, h.semaine_passee, h.employes.len()), (3.5, 77.0, 2));
        assert_eq!(
            (h.par_projet[0].nom.as_str(), h.par_projet[0].heures),
            ("Agrandissement, 12 rue des Exemples", 3.5)
        );
        let agenda: Vec<&str> = e.agenda.iter().flatten().map(|n| n.texte.as_str()).collect();
        assert_eq!(
            agenda,
            [
                "Une note de la semaine passée",
                "Réunion de chantier, 8 h",
                "Visite de chantier, 12 rue des Exemples",
                "Livraison des fermes de toit",
                "Une note dans trois semaines"
            ]
        );
        assert_eq!((e.banque.as_ref().map(Vec::len), k.solde_banque), (Some(2), Some(52010.43)));
        assert_eq!(e.alertes.as_ref().map(Vec::len), Some(5));
        assert_eq!(faux.sessions(), 0, "la session est fermée");
        assert!(l.resume().starts_with("18 outils lus sur 18 en "), "{}", l.resume());
        let journal = faux.journal().join("\n");
        for etape in [
            "mcp initialize (jeton courant)",
            "mcp notifications/initialized (jeton courant)",
            "mcp tools/call tableau_kpis (jeton courant)",
            "mcp tools/call conformite_expirations (jeton courant)",
            "mcp DELETE : session fermée",
        ] {
            assert!(journal.contains(etape), "{etape} manque :\n{journal}");
        }
        assert!(!journal.contains("creer_") && !journal.contains(&jeton), "ni écriture ni jeton :\n{journal}");
        // Un outil en panne manque sans emporter les autres ; la redirection de Starlette se prend une fois.
        faux.faire_echouer("lister_projets");
        faux.rediriger();
        let l = lire(&ressource, &jeton, &aujourdhui, false).unwrap();
        assert_eq!(l.manques, ["lister_projets"]);
        assert!(l.entreprise.projets.is_none() && l.entreprise.kpis.projets_en_cours == Some(3));
        assert_eq!(faux.journal().iter().filter(|x| x.ends_with("/mcp renvoyé vers /mcp/")).count(), 1);
        assert_eq!(faux.sessions(), 0);
        // Un jeton que le serveur ne connaît pas : refusé, rien de lu.
        assert_eq!(lire(&ressource, "eyJ.inconnu.sig", &aujourdhui, false).unwrap_err(), Panne::Jeton);
        // Un serveur absent : une panne, qui le dit.
        match lire("http://127.0.0.1:9/mcp", &jeton, &aujourdhui, false) {
            Err(Panne::Serveur(m)) => assert!(m.starts_with("Constructo AI ne répond pas"), "{m}"),
            autre => panic!("{autre:?}"),
        }
        let _ = fs::remove_file(&lien.fichier);
    }

    /// Le planning de bout en bout sur le faux Constructo AI (jamais le vrai) : une lecture avec le suivi de production
    /// (les vingt-deux outils), `entreprise.json` écrit comme le chef l'écrit, puis ce qu'en tire le centre
    /// (`Centre::planning`) — le Gantt, le Kanban et le calendrier de l'entreprise inventée.
    #[test]
    fn le_planning_de_bout_en_bout_sur_le_faux_serveur() {
        use crate::planning::{Cote, Sorte};
        let faux = crate::essais::FauxConstructo::demarrer(0, 3600, 0).unwrap();
        let (jeton, lien) = jeton_du_faux(&faux, "planning");
        let (ressource, aujourdhui) = (lien.ressource(), crate::journal::date());
        let jour = |d: i64| date_du_numero(numero_du_jour(&aujourdhui).unwrap() + d);
        let l = lire(&ressource, &jeton, &aujourdhui, true).unwrap();
        assert!(l.manques.is_empty(), "{:?}", l.manques);
        assert!(l.resume().starts_with("22 outils lus sur 22 en "), "{}", l.resume());
        let journal = faux.journal().join("\n");
        for outil in OUTILS_DU_SUIVI.iter().chain(&["suivi_colonnes", "suivi_etat_kanban", "lister_bons_travail"]) {
            assert!(journal.contains(&format!("mcp tools/call {outil} (jeton courant)")), "{outil} :\n{journal}");
        }
        let dossier = std::env::temp_dir().join(format!("eve-planning-{}", std::process::id()));
        fs::create_dir_all(&dossier).unwrap();
        ecrire(&dossier, &Entreprise::default().suite(&l, &maintenant_iso())).unwrap();
        let p = crate::centre::Centre::lu_maintenant(None, &dossier, "").planning(&aujourdhui);
        assert_eq!(p.lu.len(), 16, "{}", p.lu);

        // Le Gantt : chaque chantier (par son début), puis les siens ; un bon de travail sans chantier connu à sa
        // place ; un bon de travail dont aucune opération n'est lue (celles du bon 38 sont finies avant la fenêtre),
        // sous son chantier ; ni le bon annulé ni ses opérations ; ni l'opération sans date.
        let gantt = |cle: &str| p.gantts.iter().find(|g| g.cle == cle).unwrap_or_else(|| panic!("le Gantt {cle}"));
        let chantiers = gantt("projets");
        let ids: Vec<&str> = chantiers.taches.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "projet-9", "bt-38", "projet-14", "op-501", "op-502", "op-503", "op-504", "projet-12", "op-520",
                "op-521", "op-522", "bt-12", "op-120", "projet-16", "bt-45", "op-550",
            ]
        );
        let t = |id: &str| chantiers.taches.iter().find(|t| t.id == id).unwrap();
        assert_eq!(
            (t("projet-9").debut.clone(), t("projet-9").fin.clone()),
            (jour(-75), aujourdhui.clone()),
            "en cours"
        );
        assert_eq!((t("op-503").parent.as_deref(), t("bt-12").parent.as_deref()), (Some("projet-14"), None));
        assert_eq!((t("op-503").statut.as_str(), t("op-504").statut.as_str()), ("En attente", "En attente"));
        assert_eq!((t("op-501").avancement, t("op-520").avancement), (Some((20.0f64 / 24.0) as f32), Some(1.0)));
        assert_eq!(t("projet-14").prevu, Some((jour(-40), jour(40))), "la référence de l'ERP");
        assert_eq!(t("op-503").prevu, Some((jour(2), jour(6))));
        assert_eq!(
            (t("op-503").responsables.clone(), t("op-550").responsables.clone()),
            (vec!["Toitures Exemple".to_string()], vec!["Kim Essai".to_string()])
        );
        assert_eq!((t("bt-12").titre.as_str(), t("bt-12").statut.as_str()), ("Bon de travail BT-2026-012", "En cours"));
        let deps: Vec<(&str, &str, &str, i32)> = chantiers
            .dependances
            .iter()
            .map(|d| (d.de.as_str(), d.vers.as_str(), d.sorte.as_str(), d.decalage))
            .collect();
        assert_eq!(deps, [("op-501", "op-502", "FS", 0), ("op-502", "op-503", "FS", 1), ("op-520", "op-521", "FS", 0)]);
        // Un Gantt par tableau du suivi ; les soumissions de leurs travaux prévus (sinon, celles qui attendent leur
        // réponse, jusqu'à aujourd'hui), les ventes ouvertes depuis leur création, les bons de commande en route, les
        // factures qui attendent leur solde.
        let cles: Vec<&str> = p.gantts.iter().map(|g| g.cle.as_str()).collect();
        assert_eq!(cles, ["projets", "devis", "bons_travail", "operations", "ventes", "achats", "factures"]);
        let jours = |cle: &str| -> Vec<(String, String, String)> {
            gantt(cle).taches.iter().map(|t| (t.id.clone(), t.debut.clone(), t.fin.clone())).collect()
        };
        let j = |id: &str, d: i64, f: i64| (id.to_string(), jour(d), jour(f));
        assert_eq!(
            jours("devis"),
            [j("devis-29", -20, 0), j("devis-35", -1, 0), j("devis-33", 18, 30), j("devis-31", 25, 60)]
        );
        assert_eq!(jours("achats"), [j("bc-5", -6, 3)]);
        assert_eq!(jours("factures"), [j("facture-211", -4, 26), j("facture-212", -3, 27)]);
        let ventes = jours("ventes");
        assert_eq!(ventes.len(), 5, "les ventes ouvertes");
        assert!(ventes.iter().all(|(_, d, _)| *d == jour(-30)), "créées il y a un mois : {ventes:?}");
        // Au calendrier : chaque soumission et chaque vente, avec son tableau.
        let de = |cle: &str| p.evenements.iter().filter(|e| e.tableau.as_deref() == Some(cle)).count();
        assert_eq!((de("devis"), de("ventes")), (4, 5));

        // Le Kanban : ses tableaux dans l'ordre du planning ; dans chacun, les colonnes de l'ERP, ce qu'il y compte
        // (les variantes d'un statut réunies) et les cartes lues ; un statut hors de ses colonnes, après elles ; les
        // ventes, dans l'ordre des étapes du CRM.
        let cles: Vec<&str> = p.tableaux.iter().map(|t| t.cle.as_str()).collect();
        assert_eq!(cles, ["projets", "devis", "bons_travail", "operations", "ventes", "achats", "factures"]);
        let colonnes = |cle: &str| -> Vec<(String, usize, usize)> {
            let t = p.tableaux.iter().find(|t| t.cle == cle).unwrap();
            t.colonnes.iter().map(|c| (c.titre.clone(), c.total, c.cartes.len())).collect()
        };
        let attendu = |l: &[(&str, usize, usize)]| -> Vec<(String, usize, usize)> {
            l.iter().map(|(t, n, c)| (t.to_string(), *n, *c)).collect()
        };
        assert_eq!(
            colonnes("projets"),
            attendu(&[
                ("En attente", 1, 1),
                ("En cours", 3, 3),
                ("Terminé", 9, 0),
                ("Annulé", 1, 0),
                ("Suspendu", 0, 0)
            ])
        );
        assert_eq!(
            colonnes("bons_travail"),
            attendu(&[
                ("Brouillon", 1, 1),
                ("En cours", 3, 2),
                ("Terminé", 38, 1),
                ("En pause", 0, 0),
                ("Annulé", 2, 1)
            ])
        );
        assert_eq!(
            colonnes("operations"),
            attendu(&[("En attente", 6, 5), ("En cours", 3, 3), ("Terminé", 3, 1), ("Annulé", 1, 1)])
        );
        assert_eq!(
            colonnes("ventes"),
            attendu(&[("Qualification", 2, 2), ("Proposition", 3, 3), ("Gagné", 4, 0), ("Perdu", 2, 0)])
        );
        assert_eq!(colonnes("achats").last(), Some(&("Commandé".to_string(), 1, 1)));
        assert_eq!(colonnes("factures")[1..3].to_vec(), attendu(&[("Envoyée", 2, 2), ("Payée", 110, 0)]));
        assert_eq!(
            colonnes("devis").iter().map(|c| c.2).sum::<usize>(),
            4,
            "les soumissions en cours, une fois chacune"
        );
        let bons = p.tableaux.iter().find(|t| t.cle == "bons_travail").unwrap();
        assert!(bons.garde.as_deref().is_some_and(|g| g.starts_with("ANNULE interdit via le Kanban")));
        let vente = &p.tableaux.iter().find(|t| t.cle == "ventes").unwrap().colonnes[1].cartes[0];
        assert_eq!(
            (vente.id.as_str(), vente.montant, vente.echeance.clone()),
            ("opportunite-15", Some(180000.0), Some(jour(20)))
        );

        // Le calendrier de l'entreprise : chaque sorte de ses événements ; ni le bon annulé ni ses opérations ; une
        // livraison à son heure ; chaque chose dit à Claude ce qu'elle est et son numéro dans l'ERP.
        for s in [Sorte::Note, Sorte::Chantier, Sorte::Operation, Sorte::Facture, Sorte::Livraison, Sorte::Echeance] {
            assert!(p.evenements.iter().any(|e| e.sorte == s && e.cote == Cote::Entreprise), "{s:?}");
        }
        assert!(!p.evenements.iter().any(|e| e.titre.starts_with("Démolition")));
        let livraison =
            p.evenements.iter().find(|e| e.titre == "Livraison · Agrandissement, 12 rue des Exemples").unwrap();
        assert_eq!((livraison.debut.clone(), livraison.heure.as_deref()), (jour(3), Some("08:30")));
        let bardeaux = p.evenements.iter().find(|e| e.titre == "Bardeaux").unwrap();
        assert_eq!((bardeaux.debut.clone(), bardeaux.fin.clone()), (jour(4), Some(jour(8))));
        for quoi in ["l'opération « Bardeaux » (opération n° 503 de l'ERP)", "BT-2026-041", "Toitures Exemple"] {
            assert!(bardeaux.source.contains(quoi), "{quoi} : {}", bardeaux.source);
        }
        let mut jours: Vec<&str> = p.evenements.iter().map(|e| e.debut.as_str()).collect();
        let dans_l_ordre = jours.clone();
        jours.sort_unstable();
        assert_eq!(jours, dans_l_ordre, "dans l'ordre du calendrier");
        let _ = fs::remove_dir_all(&dossier);
        let _ = fs::remove_file(&lien.fichier);
    }

    // ---- La comptabilité ----------------------------------------------------------------------------------------

    /// Deux pages du registre, écrites à la main dans la forme du contrat : `comptabilite_lister_registre` rend un
    /// texte JSON compact, les montants en texte ; des plus récentes aux plus anciennes (`recentes_d_abord`), la
    /// première jusqu'à la facture 230, la seconde après elle.
    const PAGE_1: &str = concat!(
        r#"{"version":1,"factures":[{"id":230,"numero":"FACT-2026-00230","destinataire":"client","#,
        r#""type_document":"AVOIR","statut":"ENVOYEE","date":"2026-10-30","tiers":"Habitations Exemple","#,
        r#""sous_total":"-50.00","tps":"-2.50","tvq":"-4.99","total":null,"paye":null,"solde":"0.00"},"#,
        r#"{"numero":"sans id"}],"suivant":230,"total":4,"colonnes_absentes":["section"]}"#
    );
    const PAGE_2: &str = concat!(
        r#"{"version":1,"factures":[{"id":105,"numero":"FF-105","numero_fournisseur":"BE-40017","#,
        r#""destinataire":"fournisseur","type_document":"facture","statut":"partiellement_payee","#,
        r#""date":"2026-10-20","echeance":null,"paiement":"2026-10-28","tiers":"Béton Fictif ltée","#,
        r#""tiers_id":201,"projet_id":7,"projet":"Entrepôt Lévis","section":"2. Fondation","#,
        r#""fiabilite":"a_verifier","sous_total":"8432.07","tps":"421.60","tvq":"841.10","total":"9694.77","#,
        r#""paye":"4847.38","solde":"4847.39","taxe1":"TPS","taxe2":"TVQ","cree_le":null,"modifie_le":null},"#,
        r#"{"id":104,"numero":"FACT-2026-00104","numero_fournisseur":null,"destinataire":"client","#,
        r#""type_document":"FACTURE","statut":"ENVOYEE","date":"2026-11-01","echeance":"2026-12-01","#,
        r#""paiement":null,"tiers":"Habitations Exemple","tiers_id":12,"projet_id":7,"projet":"Entrepôt Lévis","#,
        r#""section":null,"fiabilite":null,"sous_total":"1000.00","tps":"50.00","tvq":"99.75","total":"1149.75","#,
        r#""paye":"0.00","solde":"1149.75","taxe1":"TPS","taxe2":"TVQ","cree_le":"2026-11-01T15:12:00Z","#,
        r#""modifie_le":"2026-11-02T16:04:00Z"}],"suivant":null,"total":4,"colonnes_absentes":["section"]}"#
    );
    const HISTORIQUE_1: &str = concat!(
        r#"{"version":1,"changements":[{"id":5521,"quand":"2026-11-02T16:04:00Z","qui":"luc@exemple.ca","#,
        r#""action":"paiement","entite":"facture","entite_id":105,"details":"{\"montant\":\"4847.38\"}"},"#,
        r#"{"id":5400,"quand":"2026-11-01T15:12:00Z","qui":null,"action":"creation","entite":"facture","#,
        r#""entite_id":104,"details":null}],"suivant":5400,"table_absente":false}"#
    );
    const HISTORIQUE_2: &str = concat!(
        r#"{"version":1,"changements":[{"quand":"sans id"},{"id":5399,"quand":"2026-10-30T09:00:00Z","#,
        r#""qui":"marie@exemple.ca","action":"statut","entite":"facture","entite_id":230,"#,
        r#""details":"{\"vers\":\"ENVOYEE\"}"}],"suivant":null,"table_absente":false}"#
    );

    /// Une lecture sur un serveur sans réseau : ses pages, outil par outil, dans l'ordre (une panne, aussi) ; et ce
    /// qu'Eve lui a demandé.
    fn lire_avec(
        registre: Vec<Result<&str, Panne>>,
        historique: Vec<Result<&str, Panne>>,
    ) -> (Result<LectureDeLaCompta, Panne>, Vec<(String, Value)>) {
        let (mut r, mut h) = (registre.into_iter(), historique.into_iter());
        let mut demandes = Vec::new();
        let lu = lire_les_pages(
            &mut |outil: &str, a: Value| {
                demandes.push((outil.to_string(), a));
                let page = if outil == compta::OUTIL_DU_REGISTRE { r.next() } else { h.next() };
                page.unwrap_or_else(|| Err(Panne::Serveur(format!("{outil} : plus de page")))).map(str::to_string)
            },
            PAGES,
            None,
        );
        (lu, demandes)
    }

    /// Le registre puis l'historique, page par page, des plus récents aux plus anciens — ce qu'Eve demande, ce qu'elle
    /// garde de chaque facture, au cent, dans l'ordre des id ; une ligne sans id écartée et comptée, la lecture
    /// continue.
    #[test]
    fn la_compta_se_lit_page_par_page() {
        let (lu, demandes) = lire_avec(vec![Ok(PAGE_1), Ok(PAGE_2)], vec![Ok(HISTORIQUE_1), Ok(HISTORIQUE_2)]);
        let l = lu.unwrap();
        let attendu = [
            ("comptabilite_lister_registre", json!({ "recentes_d_abord": true, "avant_id": 0, "limite": 1000 })),
            ("comptabilite_lister_registre", json!({ "recentes_d_abord": true, "avant_id": 230, "limite": 1000 })),
            ("comptabilite_lister_historique", json!({ "avant_id": 0, "limite": 500 })),
            ("comptabilite_lister_historique", json!({ "avant_id": 5400, "limite": 500 })),
        ];
        assert_eq!(demandes, attendu.map(|(o, a)| (o.to_string(), a)));
        let r = &l.registre;
        assert_eq!(r.factures.iter().map(|f| f.id).collect::<Vec<_>>(), [104, 105, 230]);
        assert_eq!((r.au_serveur, r.tronque, r.colonnes_absentes.clone()), (4, false, vec!["section".to_string()]));
        assert_eq!((l.pages, l.pages_de_l_historique, l.lignes_ecartees, l.pages_illisibles), (2, 2, 2, 0));
        // Une facture, champ par champ.
        let f = &r.factures[0];
        assert_eq!(
            (f.numero.as_str(), f.numero_fournisseur.as_deref(), f.cote, f.type_document.as_str(), f.statut.as_str()),
            ("FACT-2026-00104", None, compta::Cote::Client, "FACTURE", "ENVOYEE")
        );
        assert_eq!(
            (f.date.as_deref(), f.echeance.as_deref(), f.paiement.as_deref()),
            (Some("2026-11-01"), Some("2026-12-01"), None)
        );
        assert_eq!(
            (f.tiers.as_str(), f.tiers_id, f.projet_id, f.projet.as_deref()),
            ("Habitations Exemple", Some(12), Some(7), Some("Entrepôt Lévis"))
        );
        assert_eq!((f.section.as_deref(), f.fiabilite.as_deref()), (None, None));
        assert_eq!(
            (f.sous_total, f.tps, f.tvq, f.total, f.paye, f.solde),
            (Some(100_000), Some(5_000), Some(9_975), Some(114_975), Some(0), Some(114_975))
        );
        assert_eq!((f.taxe1.as_deref(), f.taxe2.as_deref()), (Some("TPS"), Some("TVQ")));
        assert_eq!(
            (f.cree_le.as_deref(), f.modifie_le.as_deref()),
            (Some("2026-11-01T15:12:00Z"), Some("2026-11-02T16:04:00Z"))
        );
        // Celle d'un fournisseur : son numéro à lui, sa section, sa fiabilité, payée à moitié, au cent près.
        let g = &r.factures[1];
        assert_eq!(
            (g.cote, g.numero_fournisseur.as_deref(), g.section.as_deref(), g.fiabilite.as_deref()),
            (compta::Cote::Fournisseur, Some("BE-40017"), Some("2. Fondation"), Some("a_verifier"))
        );
        assert_eq!((g.type_document.as_str(), g.statut.as_str()), ("FACTURE", "PARTIELLEMENT_PAYEE"));
        assert_eq!(
            (g.sous_total, g.tps, g.tvq, g.total, g.paye, g.solde),
            (Some(843_207), Some(42_160), Some(84_110), Some(969_477), Some(484_738), Some(484_739))
        );
        // Une note de crédit : des montants négatifs, et un total absent qui reste absent — jamais zéro.
        let a = &r.factures[2];
        assert_eq!((a.sous_total, a.tps, a.tvq, a.total, a.paye), (Some(-5_000), Some(-250), Some(-499), None, None));
        // L'historique, du plus récent au plus ancien ; personne, c'est le système (un texte vide).
        let h: Vec<(i64, &str, &str)> =
            r.historique.iter().map(|c| (c.id, c.qui.as_str(), c.action.as_str())).collect();
        assert_eq!(
            h,
            [(5521, "luc@exemple.ca", "paiement"), (5400, "", "creation"), (5399, "marie@exemple.ca", "statut")]
        );
        assert_eq!(
            (r.historique[0].entite_id, r.historique[0].details.as_str()),
            (Some(105), r#"{"montant":"4847.38"}"#)
        );
        assert!(!r.historique_tronque && !r.historique_absent);
        let resume = l.resume();
        assert!(
            resume.starts_with("3 factures en 2 pages (4 au serveur), 3 entrées d'historique en 2 pages, lue en "),
            "{resume}"
        );
        assert!(resume.ends_with(" ; 1 colonne absente ; 2 lignes écartées"), "{resume}");
        assert!(!resume.contains("Habitations") && !resume.contains("FACT-"), "des nombres seulement : {resume}");
    }

    /// Une page qui ne se lit pas arrête sa liste — dite tronquée, et comptée — sans faire tomber la lecture ; la
    /// première du registre, si : sans rien de lisible, il n'y a rien à montrer. Une panne pendant le registre — un
    /// texte d'erreur du serveur, un outil inconnu, un jeton refusé — fait manquer la lecture, au premier appel qui le
    /// dit, sans aller plus loin. Pendant l'historique, elle ne fait plus que l'arrêter : cet essai disait qu'elle
    /// faisait manquer la lecture entière, et c'était le défaut (la QA5 du lot 13.2 : une panne de l'historique seul
    /// jetait le registre lu) ; le reste dans [`une_panne_de_l_historique_garde_le_registre`].
    #[test]
    fn une_page_illisible_arrete_sa_liste_une_panne_du_registre_la_lecture() {
        let tronquee = r#"{"version":1,"factures":[{"id":104"#;
        // La deuxième page du registre ne se lit pas : la première reste, l'historique se lit.
        let (lu, demandes) = lire_avec(vec![Ok(PAGE_1), Ok(tronquee)], vec![Ok(HISTORIQUE_2)]);
        let l = lu.unwrap();
        assert_eq!((l.registre.factures.len(), l.registre.tronque, l.registre.au_serveur), (1, true, 4));
        assert_eq!((l.pages, l.pages_illisibles, l.registre.historique.len()), (1, 1, 1));
        assert_eq!(demandes.len(), 3);
        // Sans sa liste (un autre JSON), pareil ; et un `suivant` qui ne mène pas plus loin arrête la liste.
        let (lu, _) = lire_avec(vec![Ok(PAGE_1), Ok(r#"{"version":1,"total":4}"#)], vec![Ok(HISTORIQUE_2)]);
        assert!(lu.unwrap().registre.tronque);
        // La même page redonnée (son `suivant`, 230, celui qu'Eve vient de passer) : ses doublons écartés, la liste
        // arrêtée là — jamais une boucle.
        let (lu, demandes) = lire_avec(vec![Ok(PAGE_1), Ok(PAGE_1)], vec![Ok(HISTORIQUE_2)]);
        let l = lu.unwrap();
        assert_eq!((l.registre.tronque, l.pages_illisibles, demandes.len()), (true, 1, 3));
        assert_eq!(
            (l.registre.factures.len(), l.lignes_ecartees),
            (1, 4),
            "deux sans id, une en double, un changement"
        );
        // L'historique qui ne se lit pas : le registre reste, l'historique se dit tronqué — sans panne à dire.
        let (lu, _) = lire_avec(vec![Ok(PAGE_2)], vec![Ok(HISTORIQUE_1), Ok("")]);
        let l = lu.unwrap();
        assert_eq!((l.registre.historique.len(), l.registre.historique_tronque, l.pages_illisibles), (2, true, 1));
        assert_eq!(l.registre.historique_erreur, "");
        // Le journal d'audit n'existe pas encore dans cette entreprise.
        let absente = r#"{"version":1,"changements":[],"suivant":null,"table_absente":true}"#;
        let (lu, _) = lire_avec(vec![Ok(PAGE_2)], vec![Ok(absente)]);
        let r = lu.unwrap().registre;
        assert_eq!((r.historique_absent, r.historique_tronque, r.historique.len()), (true, false, 0));
        // La première page du registre ne se lit pas : la lecture manque (la dernière réussie reste, chez le chef).
        let (lu, demandes) = lire_avec(vec![Ok(tronquee)], vec![]);
        assert_eq!(lu.unwrap_err(), Panne::Serveur("Constructo AI a rendu un registre illisible".into()));
        assert_eq!(demandes.len(), 1);
        // Le texte d'erreur du serveur pendant le registre : une panne, dite telle quelle ; rien après.
        let erreur = "Erreur lors de la lecture du registre (voir logs serveur).";
        let (lu, demandes) = lire_avec(vec![Ok(PAGE_1), Ok(erreur)], vec![]);
        assert_eq!(lu.unwrap_err(), Panne::Outil(erreur.into()));
        assert_eq!(demandes.len(), 2);
        // Pendant l'historique : le registre lu reste ; l'historique s'arrête là, et la panne se dit.
        let erreur = "Erreur lors de la lecture de l'historique (voir logs serveur).";
        let (lu, demandes) = lire_avec(vec![Ok(PAGE_2)], vec![Ok(erreur)]);
        let r = lu.unwrap().registre;
        assert_eq!((r.factures.len(), r.historique.len(), r.historique_tronque, demandes.len()), (2, 0, true, 2));
        assert_eq!(
            r.historique_erreur,
            "Constructo AI n'a pas pu lire l'historique : « Erreur lors de la lecture de l'historique (voir logs \
             serveur) »."
        );
        // Un serveur d'avant la comptabilité : un seul appel, puis la phrase ; un jeton refusé en route, pareil.
        let inconnu = Panne::Inconnu(compta::OUTIL_DU_REGISTRE.into());
        let (lu, demandes) = lire_avec(vec![Err(inconnu.clone())], vec![]);
        assert_eq!((lu.unwrap_err(), demandes.len()), (inconnu, 1));
        let (lu, demandes) = lire_avec(vec![Ok(PAGE_1), Err(Panne::Jeton)], vec![]);
        assert_eq!((lu.unwrap_err(), demandes.len()), (Panne::Jeton, 2));
    }

    /// Les bornes : vingt mille factures — les PLUS RÉCENTES : la page qui les dépasse est coupée de ses plus
    /// anciennes, et la suivante n'est pas demandée (lu du plus ancien au plus récent, le registre gardait les vingt
    /// mille premières, et le mois en cours manquait : la QA5 du lot 13.2) —, mille entrées d'historique, deux cents
    /// pages — un serveur qui rendrait une facture par page ne fait pas durer la lecture des heures.
    #[test]
    fn la_compta_s_arrete_a_ses_bornes() {
        let n = compta::FACTURES_MAX as i64;
        // Trente mille factures, en pages de 1 500 de la plus récente à la plus ancienne : la quatorzième passe les
        // 20 000 ; il en reste encore après elle.
        let mut demandes = 0;
        let mut historique = 0;
        let l = lire_les_pages(
            &mut |outil: &str, a: Value| {
                if outil == compta::OUTIL_DE_L_HISTORIQUE {
                    // Des pages de 600 entrées, sans fin : 1 000 au plus.
                    historique += 1;
                    let avant = a["avant_id"].as_i64().unwrap();
                    let haut = if avant == 0 { 1_000_000 } else { avant };
                    let l: Vec<Value> = (1..=600).map(|k| json!({ "id": haut - k, "quand": "2026-11-02" })).collect();
                    return Ok(json!({ "changements": l, "suivant": haut - 600 }).to_string());
                }
                demandes += 1;
                assert_eq!(a["recentes_d_abord"], json!(true));
                let avant = a["avant_id"].as_i64().unwrap();
                // Chaque page sous la précédente, ses factures dans l'ordre des id (le contrat n'en promet pas) : la
                // borne garde quand même les plus récentes.
                let haut = if avant == 0 { 30_001 } else { avant };
                let l: Vec<Value> = (1..=1500).rev().map(|k| json!({ "id": haut - k })).collect();
                Ok(json!({ "factures": l, "suivant": haut - 1500, "total": 30_000 }).to_string())
            },
            PAGES,
            None,
        )
        .unwrap();
        let r = &l.registre;
        assert_eq!((r.factures.len(), r.tronque, r.au_serveur, demandes), (compta::FACTURES_MAX, true, 30_000, 14));
        assert_eq!((r.factures[0].id, r.factures.last().map(|f| f.id)), (10_001, Some(30_000)), "les plus récentes");
        assert_eq!((r.historique.len(), r.historique_tronque, historique), (compta::CHANGEMENTS_MAX, true, 2));
        // Juste vingt mille, et rien après : pas tronqué.
        let l = lire_les_pages(
            &mut |outil: &str, a: Value| {
                if outil == compta::OUTIL_DE_L_HISTORIQUE {
                    return Ok(r#"{"changements":[],"suivant":null}"#.into());
                }
                let avant = a["avant_id"].as_i64().unwrap();
                let haut = if avant == 0 { n + 1 } else { avant };
                let l: Vec<Value> = (1..=1000).map(|k| json!({ "id": haut - k })).collect();
                let suivant = if haut - 1000 > 1 { json!(haut - 1000) } else { Value::Null };
                Ok(json!({ "factures": l, "suivant": suivant, "total": n }).to_string())
            },
            PAGES,
            None,
        )
        .unwrap();
        assert_eq!((l.registre.factures.len(), l.registre.tronque, l.pages), (compta::FACTURES_MAX, false, 20));
        // Une facture par page, sans fin : deux cents pages, puis la liste s'arrête, tronquée.
        let mut appels = 0;
        let l = lire_les_pages(
            &mut |outil: &str, a: Value| {
                appels += 1;
                if outil == compta::OUTIL_DE_L_HISTORIQUE {
                    return Ok(r#"{"changements":[],"suivant":null}"#.into());
                }
                let avant = a["avant_id"].as_i64().unwrap();
                let suivant = if avant == 0 { 1_000_000 } else { avant - 1 };
                Ok(json!({ "factures": [{ "id": suivant }], "suivant": suivant }).to_string())
            },
            PAGES,
            None,
        )
        .unwrap();
        assert_eq!((l.pages, l.registre.factures.len(), l.registre.tronque, appels), (200, 200, true, 201));
    }

    /// L'heure d'une lecture de la comptabilité porte son décalage (« 2026-11-02T14:05:09-05:00 »), et dit l'instant
    /// présent (la QA5, passe 2 : écrite sans fuseau, son âge se comptait faux au changement d'heure).
    #[test]
    fn l_heure_d_une_lecture_porte_son_decalage() {
        let h = heure_de_la_lecture();
        let o = h.as_bytes();
        assert_eq!((h.len(), o[10], o[22]), (25, b'T', b':'), "{h}");
        assert!(matches!(o[19], b'+' | b'-'), "{h}");
        let ecart = en_unix(&h).expect("une heure lisible") - unix_maintenant();
        assert!(ecart.abs() <= 2, "{h} : {ecart} s");
    }

    /// Ce que la personne lit quand la lecture manque : chaque panne, sa phrase — celle d'un serveur d'avant la
    /// comptabilité, mot pour mot.
    #[test]
    fn chaque_panne_de_la_compta_se_dit() {
        assert_eq!(
            phrase_de_la_compta(&Panne::Inconnu("comptabilite_lister_registre".into())),
            "Constructo AI n'offre pas encore la lecture de la comptabilité : elle viendra avec sa prochaine mise à \
             jour."
        );
        assert_eq!(
            phrase_de_la_compta(&Panne::Jeton),
            "Constructo AI a refusé le jeton d'Eve : elle relira la comptabilité avec le suivant."
        );
        let profil = "comptabilite_lister_registre : L'outil « comptabilite_lister_registre » n'est pas activé pour \
                      votre entreprise. Un administrateur peut l'ajouter depuis Configuration \u{2192} MCP.";
        assert_eq!(
            phrase_de_la_compta(&Panne::Outil(profil.into())),
            "La lecture de la comptabilité n'est pas activée pour ton entreprise dans Constructo AI : un \
             administrateur peut l'ajouter depuis Configuration \u{2192} MCP."
        );
        assert_eq!(
            phrase_de_la_compta(&Panne::Outil("Erreur lors de la lecture (voir logs serveur).".into())),
            "Constructo AI n'a pas pu lire la comptabilité : « Erreur lors de la lecture (voir logs serveur) »."
        );
        assert_eq!(
            phrase_de_la_compta(&Panne::Outil("comptabilite_lister_historique : Input validation error".into())),
            "Constructo AI n'a pas pu lire la comptabilité : « Input validation error »."
        );
        assert_eq!(
            phrase_de_la_compta(&Panne::Serveur("Constructo AI ne répond pas (HTTP 503)".into())),
            "Constructo AI ne répond pas (HTTP 503)."
        );
        assert_eq!(phrase_de_la_compta(&Panne::Session), "Constructo AI a oublié la session.");
        assert_eq!(phrase_de_la_compta(&Panne::Interdit), "Constructo AI refuse l'accès à ces lectures (HTTP 403).");
        assert_eq!(
            phrase_de_la_compta(&Panne::Serveur(LECTURE_TROP_LONGUE.into())),
            "Constructo AI a mis plus de cinq minutes à rendre la comptabilité : Eve a arrêté la lecture."
        );
        // La QA5 (passe 2) : le refus d'une connexion sans entreprise (`_REG_REFUS_TENANT`), et la base occupée
        // (`_reg_message_erreur`), dits par Eve — pas le texte du serveur, sans accents, entre guillemets.
        let tenant = "comptabilite_lister_registre : Refus : aucune entreprise n'est rattachee a cette connexion ; rien \
                      n'a ete lu. Reconnecte-toi avec les identifiants de ton entreprise.";
        assert_eq!(phrase_de_la_compta(&Panne::Outil(tenant.into())), REFUS_DE_TENANT);
        assert_eq!(phrase_de_l_historique(&Panne::Outil(tenant.into())), REFUS_DE_TENANT);
        let occupee = "comptabilite_lister_registre : Erreur : la base est occupee ou la lecture du registre des \
                       factures a pris trop de temps ; rien n'a ete lu. Reessaie dans un moment.";
        assert_eq!(
            phrase_de_la_compta(&Panne::Outil(occupee.into())),
            "La base de Constructo AI était occupée : rien de la comptabilité n'a été lu. Réessaie dans un moment."
        );
        assert_eq!(
            phrase_de_l_historique(&Panne::Outil(occupee.into())),
            "La base de Constructo AI était occupée pendant la lecture de l'historique : il s'est arrêté là."
        );
        // Ce qui refuse l'accès lui-même (rien de l'entreprise ne reste alors) ; une lecture qui flanche, non.
        for p in [
            Panne::Interdit,
            Panne::Outil(profil.into()),
            Panne::Outil(tenant.into()),
            Panne::Refuse("creer_facture".into()),
        ] {
            assert!(refus_d_autorisation(&p), "{p:?}");
        }
        for p in [
            Panne::Jeton,
            Panne::Session,
            Panne::Inconnu(compta::OUTIL_DU_REGISTRE.into()),
            Panne::Serveur("Constructo AI a répondu HTTP 500".into()),
            Panne::Outil("Erreur lors de la lecture (voir logs serveur).".into()),
            Panne::Outil(occupee.into()),
        ] {
            assert!(!refus_d_autorisation(&p), "{p:?}");
        }
        // Ce qui dit un outil inconnu : FastMCP, la spécification de MCP, d'autres serveurs ; rien d'autre.
        for m in ["Unknown tool: comptabilite_lister_registre", "unknown TOOL", "Tool not found: x"] {
            assert!(outil_inconnu(m), "{m}");
        }
        for m in ["Erreur lors de la lecture (voir logs serveur).", "Invalid params", profil, ""] {
            assert!(!outil_inconnu(m), "{m}");
        }
    }

    /// Un petit registre : trois factures, deux entrées d'historique.
    fn registre_d_essai() -> compta::Registre {
        let (lu, _) = lire_avec(vec![Ok(PAGE_1), Ok(PAGE_2)], vec![Ok(HISTORIQUE_1), Ok(r#"{"changements":[]}"#)]);
        lu.unwrap().registre
    }

    /// `registre.json` : ses états (la lecture d'avant gardée, sans copie), son écriture d'un geste dans la forme de
    /// `compta::Lecture`, sa relecture ; ce qu'il a à effacer à la déconnexion ; quand il est à relire.
    #[test]
    fn le_fichier_du_registre_garde_la_lecture_d_avant() {
        let lu = FichierDuRegistre::lu(registre_d_essai(), "2026-11-02T14:05:09");
        assert_eq!((lu.etat.as_str(), lu.lu_le.as_str(), lu.erreur.as_str()), (A_JOUR, "2026-11-02T14:05:09", ""));
        let en_cours = lu.manque("une panne").en_cours();
        assert_eq!(
            (en_cours.etat.as_str(), en_cours.lu_le.as_str(), en_cours.erreur.as_str()),
            (EN_COURS, "2026-11-02T14:05:09", "")
        );
        let manque = en_cours.manque("Constructo AI ne répond pas.");
        assert_eq!((manque.etat.as_str(), manque.erreur.as_str()), (ERREUR, "Constructo AI ne répond pas."));
        // Le registre d'avant, partagé tel quel : jamais recopié.
        let (a, b, c) =
            (lu.registre.as_ref().unwrap(), en_cours.registre.as_ref().unwrap(), manque.registre.as_ref().unwrap());
        assert!(Arc::ptr_eq(a, b) && Arc::ptr_eq(a, c));
        let rien = FichierDuRegistre::non_connecte();
        assert_eq!((rien.etat.as_str(), rien.lu_le.as_str(), rien.registre.is_none()), (NON_CONNECTE, "", true));

        let dossier = std::env::temp_dir().join(format!("eve-registre-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dossier);
        fs::create_dir_all(&dossier).unwrap();
        let chemin = dossier.join(compta::FICHIER);
        assert!(FichierDuRegistre::rien_a_effacer(&dossier), "pas de fichier");
        assert_eq!(FichierDuRegistre::charger(&dossier), FichierDuRegistre::default());
        manque.ecrire(&dossier).unwrap();
        // La forme du contrat, que lisent le centre et l'onglet ; du JSON compact.
        let texte = fs::read_to_string(&chemin).unwrap();
        let tete = r#"{"etat":"erreur","lu_le":"2026-11-02T14:05:09","erreur":"Constructo AI ne répond pas.","#;
        assert!(texte.starts_with(tete) && texte.contains(r#","registre":{"factures":[{"id":104,"#), "{texte}");
        assert!(!texte.contains('\n'));
        let l: compta::Lecture = serde_json::from_str(&texte).unwrap();
        assert_eq!(l.registre.as_ref(), Some(&**a));
        assert_eq!(l.registre.unwrap().factures[0].total, Some(114_975));
        assert_eq!(FichierDuRegistre::charger(&dossier), manque);
        assert!(!FichierDuRegistre::rien_a_effacer(&dossier), "une lecture de l'entreprise : à effacer");
        rien.ecrire(&dossier).unwrap();
        assert_eq!(
            fs::read_to_string(&chemin).unwrap(),
            r#"{"etat":"non_connecte","lu_le":"","erreur":"","registre":null}"#
        );
        assert!(FichierDuRegistre::rien_a_effacer(&dossier));
        // Un fichier illisible : rien de lu, et à remplacer.
        fs::write(&chemin, b"{\"etat\":").unwrap();
        assert_eq!(FichierDuRegistre::charger(&dossier), FichierDuRegistre::default());
        assert!(!FichierDuRegistre::rien_a_effacer(&dossier));
        // Une marque d'ordre des octets en tête passe.
        let mut avec = b"\xEF\xBB\xBF".to_vec();
        avec.extend_from_slice(texte.as_bytes());
        fs::write(&chemin, &avec).unwrap();
        assert_eq!(FichierDuRegistre::charger(&dossier), manque);
        let _ = fs::remove_dir_all(&dossier);

        // À relire, à la première demande de la séance : dix minutes après sa lecture (à cinq secondes près), ou
        // quand on ne sait pas.
        assert!(!lu.a_relire("2026-11-02 14:07:09"), "deux minutes");
        assert!(!lu.a_relire("2026-11-02 14:15:03"), "9 min 54 s");
        assert!(lu.a_relire("2026-11-02 14:15:04"), "9 min 55 s");
        assert!(lu.a_relire("2026-11-03 08:00:00"), "le lendemain");
        assert!(lu.a_relire("2026-11-02 14:00:00"), "l'horloge a reculé");
        assert!(lu.en_cours().a_relire("2026-11-02 14:06:00"), "une lecture restée en cours (Eve s'est arrêtée)");
        assert!(FichierDuRegistre::default().a_relire("2026-11-02 14:06:00"), "jamais lue");
        assert!(FichierDuRegistre { lu_le: "hier".into(), ..lu.clone() }.a_relire("2026-11-02 14:06:00"));
        assert!(!lu.manque("x").a_relire("2026-11-02 14:06:00"), "une lecture manquée garde l'heure de la dernière");
    }

    /// Le fil qui écrit `registre.json` pour le chef : les états, dans l'ordre ; plusieurs qui attendent, le dernier
    /// seul s'écrit ; le chef parti, il finit ce qu'il a. Le fichier dit enfin le dernier état donné.
    #[test]
    fn le_fil_du_registre_ecrit_le_dernier_etat() {
        let dossier = std::env::temp_dir().join(format!("eve-ecrivain-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dossier);
        fs::create_dir_all(&dossier).unwrap();
        let lu = FichierDuRegistre::lu(registre_d_essai(), "2026-11-02T14:05:09");
        let ecrivain = EcrivainDuRegistre::demarrer(dossier.clone());
        for f in
            [lu.en_cours(), lu.clone(), lu.manque("Constructo AI ne répond pas."), FichierDuRegistre::non_connecte()]
        {
            ecrivain.ecrire(f);
        }
        drop(ecrivain);
        let debut = Instant::now();
        while FichierDuRegistre::charger(&dossier) != FichierDuRegistre::non_connecte()
            && debut.elapsed() < Duration::from_secs(5)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(FichierDuRegistre::charger(&dossier), FichierDuRegistre::non_connecte());
        // Un autre, qui finit sur la lecture : elle, entière.
        let ecrivain = EcrivainDuRegistre::demarrer(dossier.clone());
        ecrivain.ecrire(lu.en_cours());
        ecrivain.ecrire(lu.clone());
        drop(ecrivain);
        let debut = Instant::now();
        while FichierDuRegistre::charger(&dossier) != lu && debut.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(FichierDuRegistre::charger(&dossier), lu);
        let _ = fs::remove_dir_all(&dossier);
    }

    /// Les demandes de l'onglet, telles que le chef les mène : la première de la séance d'après `registre.json` ; puis
    /// au plus une lecture par dix minutes (depuis le début de la précédente, à cinq secondes près), « Relire » par
    /// trente secondes (plus tôt, elle attend) ; une seule à la fois ; un jeton refusé, la demande attend le suivant.
    #[test]
    fn les_demandes_de_la_compta() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let mut d = DemandesDeLaCompta::default();
        d.vouloir(false, s(0), || false);
        assert!(!d.voulue(), "registre.json lu il y a moins de dix minutes : rien");
        d.vouloir(false, s(0), || true);
        assert_eq!(d.partir(s(0), 1), Some(false));
        // Pendant la lecture : rien ne s'ajoute, elle ne part pas deux fois.
        d.vouloir(true, s(1), || true);
        assert!(!d.voulue());
        assert_eq!(d.partir(s(2), 1), None);
        d.finie(FinDeLecture::Lue, false, s(3));
        // L'onglet redemande toutes les deux minutes : rien avant les dix minutes ; `registre.json` n'est plus
        // consulté.
        for n in [120, 240, 360, 480, 594] {
            d.vouloir(false, s(n), || panic!("la séance a sa lecture"));
            assert!(!d.voulue(), "{n} s");
        }
        d.vouloir(false, s(595), || false);
        assert_eq!(d.partir(s(595), 1), Some(false), "9 min 55 s : la cinquième demande de l'onglet");
        d.finie(FinDeLecture::Manquee, false, s(600));
        // « Relire » dix secondes après le début : il attend la fin de ses trente secondes, puis part.
        d.vouloir(true, s(605), || false);
        d.vouloir(false, s(610), || true);
        assert_eq!(d.partir(s(624), 1), None);
        assert_eq!(d.partir(s(625), 1), Some(true));
        d.finie(FinDeLecture::Lue, true, s(626));
        // « Relire » plus de trente secondes après : aussitôt.
        d.vouloir(true, s(660), || false);
        assert_eq!(d.partir(s(660), 1), Some(true));
        // Un jeton refusé : la demande revient, et attend le jeton suivant — aussitôt qu'il est là.
        d.finie(FinDeLecture::JetonRefuse(1), true, s(661));
        assert!(d.voulue());
        assert_eq!(d.partir(s(700), 1), None, "le jeton refusé : non");
        assert_eq!(d.partir(s(700), 2), Some(true), "le suivant : oui");
        d.finie(FinDeLecture::Lue, true, s(701));
        // La connexion s'en va : plus rien n'attend ; la séance suivante repart de registre.json.
        d.vouloir(true, s(800), || false);
        assert!(d.voulue());
        d.oublier();
        assert!(!d.voulue());
        assert_eq!(d.partir(s(900), 2), None);
        d.vouloir(false, s(901), || false);
        assert!(!d.voulue());
        d.vouloir(false, s(901), || true);
        assert_eq!(d.partir(s(901), 3), Some(false));
    }

    /// Une panne pendant l'historique — le réseau, le jeton refusé, un outil que le serveur n'a pas, la lecture qu'un
    /// administrateur n'a pas ouverte — ne jette plus le registre lu : l'historique s'arrête là, tronqué, et elle se
    /// dit (`historique_erreur`) ; un jeton refusé se dit aussi au chef, qui le fait renouveler. Un 403, lui, refuse
    /// l'accès à tout : la lecture manque (la QA5 du lot 13.2).
    #[test]
    fn une_panne_de_l_historique_garde_le_registre() {
        let reseau = Panne::Serveur("Constructo AI ne répond pas : le serveur ne répond pas à temps".into());
        let (lu, _) = lire_avec(vec![Ok(PAGE_2)], vec![Ok(HISTORIQUE_1), Err(reseau)]);
        let l = lu.unwrap();
        let r = &l.registre;
        assert_eq!((r.factures.len(), r.historique.len(), r.historique_tronque, l.jeton_refuse), (2, 2, true, false));
        assert_eq!(
            r.historique_erreur,
            "L'historique s'est arrêté avant sa fin — Constructo AI ne répond pas : le serveur ne répond pas à temps."
        );
        let (lu, _) = lire_avec(vec![Ok(PAGE_2)], vec![Err(Panne::Jeton)]);
        let l = lu.unwrap();
        assert_eq!((l.registre.factures.len(), l.registre.historique_tronque, l.jeton_refuse), (2, true, true));
        assert_eq!(
            l.registre.historique_erreur,
            "Constructo AI a refusé le jeton d'Eve pendant la lecture de l'historique : il se relira avec le suivant."
        );
        let (lu, _) = lire_avec(vec![Ok(PAGE_2)], vec![Err(Panne::Inconnu(compta::OUTIL_DE_L_HISTORIQUE.into()))]);
        assert_eq!(
            lu.unwrap().registre.historique_erreur,
            "Constructo AI n'offre pas encore l'historique de la comptabilité : il viendra avec sa prochaine mise à \
             jour."
        );
        let profil = "comptabilite_lister_historique : L'outil « comptabilite_lister_historique » n'est pas activé \
                      pour votre entreprise. Un administrateur peut l'ajouter depuis Configuration \u{2192} MCP.";
        let (lu, _) = lire_avec(vec![Ok(PAGE_2)], vec![Err(Panne::Outil(profil.into()))]);
        assert_eq!(
            lu.unwrap().registre.historique_erreur,
            "La lecture de l'historique n'est pas activée pour ton entreprise dans Constructo AI : un administrateur \
             peut l'ajouter depuis Configuration \u{2192} MCP."
        );
        let (lu, _) = lire_avec(vec![Ok(PAGE_2)], vec![Err(Panne::Interdit)]);
        assert_eq!(lu.unwrap_err(), Panne::Interdit);
    }

    /// La lecture a son échéance : passée pendant le registre, la lecture manque et le dit ; passée pendant
    /// l'historique, il s'arrête là et le dit, le registre gardé. Rien ne se demande plus après elle (la QA5 du lot
    /// 13.2 : un serveur qui rendait des pages sans fin, ou qui ne répondait plus qu'en « : ping », tenait la lecture
    /// des heures).
    #[test]
    fn la_lecture_a_son_echeance() {
        // Un serveur qui rend une facture par page, une page par 60 ms, sans fin : l'échéance à 150 ms l'arrête.
        let echeance = Instant::now() + Duration::from_millis(150);
        let mut appels = 0;
        let lu = lire_les_pages(
            &mut |_: &str, a: Value| {
                appels += 1;
                std::thread::sleep(Duration::from_millis(60));
                let avant = a["avant_id"].as_i64().unwrap();
                let haut = if avant == 0 { 1_000_000 } else { avant };
                Ok(json!({ "factures": [{ "id": haut - 1 }], "suivant": haut - 1 }).to_string())
            },
            PAGES,
            Some(echeance),
        );
        assert_eq!(lu.unwrap_err(), Panne::Serveur(LECTURE_TROP_LONGUE.into()));
        assert!(appels <= 5, "{appels} appels");
        // Pendant l'historique : le registre lu reste ; l'historique s'arrête, et le dit.
        let echeance = Instant::now() + Duration::from_millis(150);
        let lu = lire_les_pages(
            &mut |outil: &str, a: Value| {
                if outil == compta::OUTIL_DU_REGISTRE {
                    return Ok(PAGE_2.to_string());
                }
                std::thread::sleep(Duration::from_millis(60));
                let avant = a["avant_id"].as_i64().unwrap();
                let haut = if avant == 0 { 1_000_000 } else { avant };
                let entree = json!({ "id": haut - 1, "quand": "2026-11-02T16:04:00Z" });
                Ok(json!({ "changements": [entree], "suivant": haut - 1 }).to_string())
            },
            PAGES,
            Some(echeance),
        );
        let r = lu.unwrap().registre;
        assert_eq!((r.factures.len(), r.historique_tronque), (2, true));
        assert!(r.historique.len() <= 5, "{} entrées", r.historique.len());
        assert_eq!(
            r.historique_erreur,
            "Constructo AI a mis plus de cinq minutes à rendre la comptabilité : l'historique s'est arrêté là."
        );
    }

    /// Un serveur sans les outils de la comptabilité n'est relu par les demandes ordinaires qu'une fois par heure — rien
    /// ne les fera venir d'ici dix minutes, mais sa mise à jour les apportera (la QA5, passe 2 : il ne l'était plus de
    /// la séance, et l'onglet disait « n'offre pas encore » après la mise à jour de l'ERP) — ; « Relire » essaie
    /// encore, et une lecture qui réussit fait reprendre les dix minutes. Une panne ordinaire ne les ralentit pas ; une
    /// nouvelle séance (une reconnexion) essaie de nouveau (la QA5 du lot 13.2).
    #[test]
    fn un_serveur_sans_les_outils_n_est_relu_qu_une_fois_par_heure() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let mut d = DemandesDeLaCompta::default();
        d.vouloir(false, s(0), || true);
        assert_eq!(d.partir(s(0), 1), Some(false));
        d.finie(FinDeLecture::Absente, false, s(1));
        for n in [600, 3_000, 3_594] {
            d.vouloir(false, s(n), || true);
            assert!(!d.voulue(), "{n} s");
        }
        // Une heure après le début de la lecture d'avant, à la marge près : la demande ordinaire part.
        d.vouloir(false, s(3_595), || true);
        assert_eq!(d.partir(s(3_595), 1), Some(false));
        d.finie(FinDeLecture::Absente, false, s(3_596));
        d.vouloir(false, s(4_300), || true);
        assert!(!d.voulue(), "toujours sans les outils : l'heure d'après");
        d.vouloir(true, s(4_301), || true);
        assert_eq!(d.partir(s(4_301), 1), Some(true));
        d.finie(FinDeLecture::Lue, true, s(4_302));
        d.vouloir(false, s(4_901), || true);
        assert!(d.voulue(), "lue : les demandes ordinaires reprennent");
        let mut d = DemandesDeLaCompta::default();
        d.vouloir(false, s(0), || true);
        assert_eq!(d.partir(s(0), 1), Some(false));
        d.finie(FinDeLecture::Manquee, false, s(1));
        d.vouloir(false, s(600), || true);
        assert!(d.voulue(), "une panne ordinaire : la suivante à dix minutes");
        let mut d = DemandesDeLaCompta::default();
        d.vouloir(false, s(0), || true);
        assert_eq!(d.partir(s(0), 1), Some(false));
        d.finie(FinDeLecture::Absente, false, s(1));
        d.oublier();
        d.vouloir(false, s(2), || true);
        assert!(d.voulue(), "une nouvelle séance");
    }

    // ---- La comptabilité du chef (`LaCompta`) : ses gardes, une à une ------------------------------------------

    /// Une connexion faite, son jeton prêt : l'émission `emis`, le tour `tour`.
    fn reliee(emis: u64, tour: u64) -> EtatDeLaConnexion {
        EtatDeLaConnexion { reliee: true, occupee: false, acces_valable: true, a_renouveler: false, emis, tour }
    }

    /// Les écritures de `registre.json` parmi ce que demande `LaCompta`.
    fn ecrits(ordres: &[PourLaCompta]) -> Vec<&FichierDuRegistre> {
        ordres
            .iter()
            .filter_map(|o| match o {
                PourLaCompta::Ecrire(f) => Some(f),
                _ => None,
            })
            .collect()
    }

    /// Une lecture qui réussit, sans réseau : le petit registre.
    fn lecture_d_essai() -> LectureDeLaCompta {
        LectureDeLaCompta { registre: registre_d_essai(), ..LectureDeLaCompta::default() }
    }

    const HEURE: &str = "2026-11-02 14:05:09";

    /// `LaCompta` menée jusqu'à une lecture qui part : une demande ordinaire, `registre.json` chargé (`f`), un
    /// battement ; rend ce que demande le battement.
    fn jusqu_a_la_lecture(
        la: &mut LaCompta,
        f: FichierDuRegistre,
        c: &EtatDeLaConnexion,
        t: Instant,
    ) -> Vec<PourLaCompta> {
        assert_eq!(la.demande(false, t, HEURE, c), vec![PourLaCompta::Charger { tour: c.tour }]);
        assert_eq!(la.charge(f, c.tour, t, HEURE, c), vec![]);
        la.battement(t, c)
    }

    /// Une lecture de l'entreprise A qui revient après une déconnexion, puis une connexion à B, est écartée : jamais la
    /// comptabilité de A dans le `registre.json` de B ; celle de B reste en cours, une seule à la fois (la QA5 du lot
    /// 13.2 : la garde du chef n'était essayée par rien).
    #[test]
    fn une_lecture_d_avant_la_deconnexion_est_ecartee() {
        let t = Instant::now();
        let mut la = LaCompta::default();
        let a = reliee(1, 1);
        let ordres = jusqu_a_la_lecture(&mut la, FichierDuRegistre::default(), &a, t);
        assert!(ordres.contains(&PourLaCompta::Lire { tour: 1, emis: 1, relire: false }), "{ordres:?}");
        la.sans_connexion(|| false);
        let b = reliee(5, 2);
        assert!(la.demande(false, t, HEURE, &b).is_empty(), "registre.json effacé : rien à charger");
        assert!(la.battement(t, &b).contains(&PourLaCompta::Lire { tour: 2, emis: 5, relire: false }));
        // La lecture de A revient : écartée.
        let ordres = la.lue(1, 1, false, Duration::from_secs(3), Ok(lecture_d_essai()), t, "2026-11-02T14:05:12", &b);
        assert!(ecrits(&ordres).is_empty(), "{ordres:?}");
        assert_eq!(la.fichier.as_ref().map(|f| f.etat.as_str()), Some(EN_COURS));
        let plus_tard = t + Duration::from_secs(60);
        la.demande(true, plus_tard, HEURE, &b);
        assert!(la.battement(plus_tard, &b).is_empty(), "celle de B se fait encore");
        // Celle de B : prise.
        let ordres = la.lue(2, 5, false, Duration::from_secs(3), Ok(lecture_d_essai()), t, "2026-11-02T14:05:12", &b);
        assert_eq!(ecrits(&ordres).iter().map(|f| f.etat.as_str()).collect::<Vec<_>>(), [A_JOUR]);
    }

    /// La lecture ne part qu'avec un jeton prêt : jamais pendant un renouvellement, ni avec un jeton qui ne vaut plus
    /// ou qui arrive à échéance ; dès qu'il l'est, elle part (la QA5 du lot 13.2 : la garde du chef n'était essayée
    /// par rien).
    #[test]
    fn la_lecture_attend_un_jeton_pret() {
        let t = Instant::now();
        let mut la = LaCompta::default();
        let pret = reliee(1, 1);
        la.demande(false, t, HEURE, &pret);
        la.charge(FichierDuRegistre::default(), 1, t, HEURE, &pret);
        for (c, quoi) in [
            (EtatDeLaConnexion { occupee: true, ..pret }, "un renouvellement en cours"),
            (EtatDeLaConnexion { acces_valable: false, ..pret }, "un jeton qui ne vaut plus"),
            (EtatDeLaConnexion { a_renouveler: true, ..pret }, "un jeton qui arrive à échéance"),
        ] {
            assert!(la.battement(t, &c).is_empty(), "{quoi}");
        }
        let ordres = la.battement(t, &pret);
        assert!(ordres.contains(&PourLaCompta::Lire { tour: 1, emis: 1, relire: false }), "{ordres:?}");
    }

    /// Sans connexion, `registre.json` ne garde rien de l'entreprise : la déconnexion l'efface, un battement sans
    /// connexion aussi s'il restait quelque chose ; un fichier qui ne dit déjà que cela ne se réécrit pas (la QA5 du
    /// lot 13.2 : la garde du chef n'était essayée par rien).
    #[test]
    fn la_deconnexion_efface_la_compta() {
        let t = Instant::now();
        let c = reliee(1, 1);
        let lue = |la: &mut LaCompta| {
            jusqu_a_la_lecture(la, FichierDuRegistre::default(), &c, t);
            la.lue(1, 1, false, Duration::from_secs(3), Ok(lecture_d_essai()), t, "2026-11-02T14:05:12", &c)
        };
        let effacer = vec![PourLaCompta::Ecrire(FichierDuRegistre::non_connecte())];
        let mut la = LaCompta::default();
        assert_eq!(ecrits(&lue(&mut la)).len(), 1);
        assert_eq!(la.sans_connexion(|| panic!("le fichier est chargé : rien à demander au disque")), effacer);
        assert!(la.sans_connexion(|| true).is_empty(), "déjà effacé : rien à réécrire");
        assert!(la.battement(t, &EtatDeLaConnexion::default()).is_empty());
        // Un battement sans connexion efface aussi ce qui restait.
        let mut la = LaCompta::default();
        lue(&mut la);
        assert_eq!(la.battement(t, &EtatDeLaConnexion { reliee: false, ..c }), effacer);
        assert!(la.battement(t, &EtatDeLaConnexion { reliee: false, ..c }).is_empty(), "une fois");
        // Pas chargé : le disque en décide.
        assert_eq!(LaCompta::default().sans_connexion(|| false), effacer);
        assert!(LaCompta::default().sans_connexion(|| true).is_empty());
    }

    /// Un jeton refusé (un 401) fait renouveler celui de son émission, et la demande attend le suivant ; la lecture
    /// d'avant reste, avec sa phrase. Pendant l'historique aussi (le registre lu, lui, reste). Une autre panne ne fait
    /// rien renouveler (la QA5 du lot 13.2 : la garde du chef n'était essayée par rien).
    #[test]
    fn un_jeton_refuse_fait_renouveler() {
        let t = Instant::now();
        let c = reliee(7, 1);
        let mut la = LaCompta::default();
        jusqu_a_la_lecture(&mut la, FichierDuRegistre::lu(registre_d_essai(), "2026-11-02T13:00:00"), &c, t);
        let ordres = la.lue(1, 7, false, Duration::from_secs(1), Err(Panne::Jeton), t, "2026-11-02T14:05:10", &c);
        assert!(ordres.contains(&PourLaCompta::Renouveler { emis: 7 }), "{ordres:?}");
        let f = ecrits(&ordres)[0];
        assert_eq!((f.etat.as_str(), f.registre.is_some(), f.lu_le.as_str()), (ERREUR, true, "2026-11-02T13:00:00"));
        assert_eq!(f.erreur, phrase_de_la_compta(&Panne::Jeton));
        assert!(la.battement(t, &c).is_empty(), "pas avec le jeton refusé");
        let suivant = reliee(8, 1);
        assert!(la.battement(t, &suivant).contains(&PourLaCompta::Lire { tour: 1, emis: 8, relire: false }));
        let pendant = LectureDeLaCompta { jeton_refuse: true, ..lecture_d_essai() };
        let ordres = la.lue(1, 8, false, Duration::from_secs(1), Ok(pendant), t, "2026-11-02T14:05:12", &suivant);
        assert!(ordres.contains(&PourLaCompta::Renouveler { emis: 8 }), "{ordres:?}");
        assert_eq!(ecrits(&ordres)[0].etat, A_JOUR);
        let mut la = LaCompta::default();
        jusqu_a_la_lecture(&mut la, FichierDuRegistre::default(), &c, t);
        let panne = Panne::Serveur("Constructo AI a répondu HTTP 500".into());
        let ordres = la.lue(1, 7, false, Duration::from_secs(1), Err(panne), t, "2026-11-02T14:05:10", &c);
        assert!(!ordres.iter().any(|o| matches!(o, PourLaCompta::Renouveler { .. })), "{ordres:?}");
    }

    /// Un refus d'accès — un 403, la lecture qu'un administrateur n'a pas ouverte à l'entreprise — n'en laisse rien :
    /// ni la lecture d'avant, ni son heure, à l'écran comme dans `registre.json`, que lit Claude. Une panne ordinaire
    /// garde la lecture d'avant (la QA5 du lot 13.2).
    #[test]
    fn un_refus_d_acces_ne_garde_rien() {
        let t = Instant::now();
        let c = reliee(1, 1);
        let avant = || FichierDuRegistre::lu(registre_d_essai(), "2026-11-02T13:00:00");
        let profil = "comptabilite_lister_registre : L'outil « comptabilite_lister_registre » n'est pas activé pour \
                      votre entreprise. Un administrateur peut l'ajouter depuis Configuration \u{2192} MCP.";
        for panne in [Panne::Interdit, Panne::Outil(profil.into())] {
            let mut la = LaCompta::default();
            jusqu_a_la_lecture(&mut la, avant(), &c, t);
            let attendu = FichierDuRegistre::refusee(&phrase_de_la_compta(&panne));
            assert_eq!((attendu.etat.as_str(), attendu.registre.is_none(), attendu.lu_le.as_str()), (ERREUR, true, ""));
            let ordres = la.lue(1, 1, false, Duration::from_secs(1), Err(panne), t, "2026-11-02T14:05:10", &c);
            assert_eq!(ecrits(&ordres), vec![&attendu]);
        }
        let mut la = LaCompta::default();
        jusqu_a_la_lecture(&mut la, avant(), &c, t);
        let panne = Panne::Serveur("Constructo AI ne répond pas (HTTP 503)".into());
        let ordres = la.lue(1, 1, false, Duration::from_secs(1), Err(panne), t, "2026-11-02T14:05:10", &c);
        let f = ecrits(&ordres)[0];
        assert_eq!((f.etat.as_str(), f.registre.is_some(), f.lu_le.as_str()), (ERREUR, true, "2026-11-02T13:00:00"));
    }

    /// Un serveur sans les outils de la comptabilité : l'onglet qui redemande toutes les deux minutes ne fait rien
    /// partir avant l'heure ; « Relire » essaie encore (la QA5 du lot 13.2) ; une heure après la dernière lecture, une
    /// demande ordinaire repart — la mise à jour de l'ERP a pu les apporter (la QA5, passe 2 : plus jamais de la
    /// séance).
    #[test]
    fn sans_les_outils_une_lecture_ordinaire_par_heure() {
        let t = Instant::now();
        let s = |n: u64| t + Duration::from_secs(n);
        let c = reliee(1, 1);
        let mut la = LaCompta::default();
        jusqu_a_la_lecture(&mut la, FichierDuRegistre::default(), &c, t);
        let inconnu = || Panne::Inconnu(compta::OUTIL_DU_REGISTRE.into());
        let ordres = la.lue(1, 1, false, Duration::from_secs(1), Err(inconnu()), s(1), "2026-11-02T14:05:10", &c);
        assert_eq!(ecrits(&ordres)[0].erreur, COMPTA_PAS_ENCORE);
        for n in (120..=480).step_by(120) {
            la.demande(false, s(n), HEURE, &c);
            assert!(la.battement(s(n), &c).is_empty(), "{n} s");
        }
        la.demande(true, s(601), HEURE, &c);
        assert!(la.battement(s(601), &c).contains(&PourLaCompta::Lire { tour: 1, emis: 1, relire: true }));
        la.lue(1, 1, true, Duration::from_secs(1), Err(inconnu()), s(602), "2026-11-02T14:15:10", &c);
        for n in (721..=4_081).step_by(120) {
            la.demande(false, s(n), HEURE, &c);
            assert!(la.battement(s(n), &c).is_empty(), "{n} s");
        }
        la.demande(false, s(4_201), HEURE, &c);
        assert!(la.battement(s(4_201), &c).contains(&PourLaCompta::Lire { tour: 1, emis: 1, relire: false }));
    }

    /// `registre.json` se charge dans un fil, une fois, à la première demande — vingt mille factures, un dixième de
    /// seconde que le chef ne passe pas à attendre (la QA5 du lot 13.2) ; la demande l'attend, puis se fait ; un
    /// chargement d'avant une déconnexion est écarté.
    #[test]
    fn le_registre_se_charge_hors_du_chef() {
        let t = Instant::now();
        let c = reliee(1, 1);
        let mut la = LaCompta::default();
        assert_eq!(la.demande(false, t, HEURE, &c), vec![PourLaCompta::Charger { tour: 1 }]);
        assert!(la.demande(true, t, HEURE, &c).is_empty(), "un seul chargement");
        assert!(la.battement(t, &c).is_empty(), "pas avant le fichier");
        // Chargé : la demande qui l'attendait se fait — « Relire », la plus forte des deux.
        let frais = FichierDuRegistre::lu(registre_d_essai(), "2026-11-02T14:04:00");
        assert!(la.charge(frais.clone(), 1, t, HEURE, &c).is_empty());
        assert!(la.battement(t, &c).contains(&PourLaCompta::Lire { tour: 1, emis: 1, relire: true }));
        // Une demande ordinaire seule, sur un fichier lu il y a une minute : rien.
        let mut la = LaCompta::default();
        la.demande(false, t, HEURE, &c);
        la.charge(frais.clone(), 1, t, HEURE, &c);
        assert!(la.battement(t, &c).is_empty());
        // Un chargement qui revient après une déconnexion : écarté ; le fichier reste effacé.
        let mut la = LaCompta::default();
        la.demande(false, t, HEURE, &c);
        la.sans_connexion(|| false);
        assert!(ecrits(&la.charge(frais, 1, t, HEURE, &c)).is_empty());
        assert_eq!(la.fichier, Some(FichierDuRegistre::non_connecte()));
    }

    // ---- Les essais qui manquaient à la lecture (la QA5 du lot 13.2, mutation, passe 1) ----------------------------
    //
    // Écrits par la QA contre la lecture du plus ancien au plus récent : ceux du registre redonnent ici leurs pages des
    // plus récentes aux plus anciennes (`avant_id`), comme le lit Eve maintenant ; ce qu'ils jugent reste le même.

    /// Une lecture sur un serveur inventé : la page du registre avant `avant_id` (des plus récentes aux plus
    /// anciennes), celle de l'historique avant `avant_id` ; et combien d'appels à chaque outil.
    fn lire_un_serveur(
        mut registre: impl FnMut(i64) -> Value,
        mut historique: impl FnMut(i64) -> Value,
    ) -> (LectureDeLaCompta, usize, usize) {
        let (mut nr, mut nh) = (0, 0);
        let l = lire_les_pages(
            &mut |outil: &str, a: Value| {
                if outil == compta::OUTIL_DU_REGISTRE {
                    nr += 1;
                    assert_eq!(a["recentes_d_abord"], json!(true));
                    Ok(registre(a["avant_id"].as_i64().unwrap()).to_string())
                } else {
                    nh += 1;
                    Ok(historique(a["avant_id"].as_i64().unwrap()).to_string())
                }
            },
            PAGES,
            None,
        )
        .unwrap();
        (l, nr, nh)
    }

    fn un_registre(_: i64) -> Value {
        json!({ "factures": [{ "id": 1 }], "suivant": null, "total": 1 })
    }

    fn aucun_historique(_: i64) -> Value {
        json!({ "changements": [], "suivant": null })
    }

    /// Une entrée d'historique que redonne la page suivante s'écarte (le mutant 509 la gardait) ; un `suivant` qui ne
    /// recule pas arrête l'historique, tronqué, la page comptée illisible (le mutant 510 le suivait).
    #[test]
    fn une_entree_redonnee_et_un_suivant_qui_ne_recule_pas() {
        let (l, _, _) = lire_un_serveur(un_registre, |avant| {
            if avant == 0 {
                json!({ "changements": [{ "id": 10 }, { "id": 9 }], "suivant": 9 })
            } else {
                json!({ "changements": [{ "id": 9 }, { "id": 8 }], "suivant": null })
            }
        });
        assert_eq!(l.registre.historique.iter().map(|c| c.id).collect::<Vec<_>>(), [10, 9, 8]);
        let (l, _, nh) = lire_un_serveur(un_registre, |avant| {
            if avant == 0 {
                json!({ "changements": [{ "id": 10 }, { "id": 9 }], "suivant": 9 })
            } else {
                json!({ "changements": [{ "id": 8 }], "suivant": 9 })
            }
        });
        assert_eq!((nh, l.registre.historique_tronque, l.pages_illisibles), (2, true, 1));
    }

    /// Vingt pages d'historique au plus (le mutant 511 en lisait vingt et une).
    #[test]
    fn vingt_pages_d_historique_au_plus() {
        let (l, _, nh) = lire_un_serveur(un_registre, |avant| {
            let haut = if avant == 0 { 1_000 } else { avant };
            json!({ "changements": [{ "id": haut - 1 }], "suivant": haut - 1 })
        });
        assert_eq!((nh, l.registre.historique_tronque), (20, true));
    }

    /// Juste à la borne, par pages du serveur : mille entrées en deux pages, la dernière sans `suivant`, ne sont pas
    /// tronquées (le mutant 512 le disait toujours) ; vingt mille factures en vingt pages pleines, d'autres après : la
    /// vingt et unième n'est pas demandée (le mutant 502 la demandait) ; mille entrées, d'autres après : tronqué, sans
    /// troisième page (le mutant 513 la demandait).
    #[test]
    fn juste_a_la_borne_par_pages_du_serveur() {
        let (l, _, nh) = lire_un_serveur(un_registre, |avant| {
            let haut = if avant == 0 { 2_000 } else { avant };
            let lignes: Vec<Value> = (1..=500).map(|k| json!({ "id": haut - k })).collect();
            let suivant = if avant == 0 { json!(haut - 500) } else { Value::Null };
            json!({ "changements": lignes, "suivant": suivant })
        });
        assert_eq!((l.registre.historique.len(), l.registre.historique_tronque, nh), (1_000, false, 2));
        let (l, nr, _) = lire_un_serveur(
            |avant| {
                let haut = if avant == 0 { 30_001 } else { avant };
                let lignes: Vec<Value> = (1..=1_000).map(|k| json!({ "id": haut - k })).collect();
                json!({ "factures": lignes, "suivant": haut - 1_000, "total": 30_000 })
            },
            aucun_historique,
        );
        assert_eq!((l.registre.factures.len(), l.registre.tronque, nr), (compta::FACTURES_MAX, true, 20));
        let (l, _, nh) = lire_un_serveur(un_registre, |avant| {
            let haut = if avant == 0 { 5_000 } else { avant };
            let lignes: Vec<Value> = (1..=500).map(|k| json!({ "id": haut - k })).collect();
            json!({ "changements": lignes, "suivant": haut - 500 })
        });
        assert_eq!((l.registre.historique.len(), l.registre.historique_tronque, nh), (1_000, true, 2));
    }

    /// Un serveur qui ne dit pas son `total` : les factures lues le disent (le mutant 518 disait zéro) ; un `suivant`
    /// écrit en texte se lit (le mutant 543 arrêtait la liste).
    #[test]
    fn un_serveur_sans_total_et_un_suivant_ecrit_en_texte() {
        let (l, nr, _) = lire_un_serveur(
            |avant| {
                if avant == 0 {
                    json!({ "factures": [{ "id": 3 }, { "id": 2 }], "suivant": "2" })
                } else {
                    json!({ "factures": [{ "id": 1 }], "suivant": null })
                }
            },
            aucun_historique,
        );
        assert_eq!((nr, l.registre.factures.len(), l.registre.tronque, l.registre.au_serveur), (2, 3, false, 3));
    }

    /// « Relire » en début de séance part, `registre.json` frais ou non (le mutant 528 attendait) ; deux demandes qui
    /// attendent se rejoignent — la plus proche, et « Relire » si l'une l'est (le mutant 529 gardait la seconde).
    #[test]
    fn relire_en_debut_de_seance_et_deux_demandes() {
        let t0 = Instant::now();
        let s = |n: u64| t0 + Duration::from_secs(n);
        let mut d = DemandesDeLaCompta::default();
        d.vouloir(true, s(0), || false);
        assert_eq!(d.partir(s(0), 1), Some(true));
        let mut d = DemandesDeLaCompta::default();
        d.vouloir(false, s(0), || true);
        assert_eq!(d.partir(s(0), 1), Some(false));
        d.finie(FinDeLecture::Lue, false, s(1));
        d.vouloir(false, s(600), || panic!("la séance a sa lecture"));
        d.vouloir(true, s(601), || panic!("la séance a sa lecture"));
        assert_eq!(d.partir(s(600), 1), Some(true));
    }

    /// Une panne qui finit par un point n'en prend pas un second (le mutant 552 en ajoutait toujours un).
    #[test]
    fn une_panne_qui_finit_par_un_point_n_en_prend_pas_deux() {
        let p = Panne::Serveur("Constructo AI ne répond pas.".into());
        assert_eq!(phrase_de_la_compta(&p), "Constructo AI ne répond pas.");
        assert_eq!(phrase_de_l_historique(&p), "L'historique s'est arrêté avant sa fin — Constructo AI ne répond pas.");
    }

    /// Un gros `registre.json` (plus de 4 Kio : une lecture de l'entreprise) est à effacer à la déconnexion, sans le
    /// lire en entier (le mutant 562 le laissait).
    #[test]
    fn un_gros_registre_json_est_a_effacer() {
        let dossier = std::env::temp_dir().join(format!("eve-qa-gros-registre-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dossier);
        fs::create_dir_all(&dossier).unwrap();
        let factures = (1..=50)
            .map(|k| compta::Facture {
                id: k,
                numero: format!("FACT-2026-{k:05}"),
                tiers: "Habitations Exemple".into(),
                ..compta::Facture::default()
            })
            .collect();
        let registre = compta::Registre { factures, ..compta::Registre::default() };
        FichierDuRegistre::lu(registre, "2026-11-02T14:05:09").ecrire(&dossier).unwrap();
        assert!(fs::metadata(dossier.join(compta::FICHIER)).unwrap().len() > 4096);
        assert!(!FichierDuRegistre::rien_a_effacer(&dossier));
        let _ = fs::remove_dir_all(&dossier);
    }

    /// La comptabilité de bout en bout sur le faux Constructo AI (jamais le vrai) : la session, le registre de la
    /// vitrine lu par pages (de 25 et de 15 : plusieurs, comme une grande entreprise en pages de 1000 et de 500),
    /// chaque facture telle quelle, des montants vérifiés au cent ; puis `registre.json` écrit comme le chef l'écrit,
    /// et relu par le fil du centre, pour l'onglet.
    #[test]
    fn la_compta_de_bout_en_bout_sur_le_faux_serveur() {
        let faux = crate::essais::FauxConstructo::demarrer(0, 3600, 0).unwrap();
        let (jeton, lien) = jeton_du_faux(&faux, "compta");
        let attendu = compta::vitrine::registre(&crate::journal::date());
        let pages = Pages { registre: 25, historique: 15 };
        let l = lire_la_compta(&lien.ressource(), &jeton, pages, COMPTA_DUREE_MAX).unwrap();
        let r = &l.registre;
        let n = attendu.factures.len();
        assert!(n > 100, "la vitrine a-t-elle changé ? {n} factures");
        assert_eq!((l.pages, l.pages_de_l_historique), (n.div_ceil(25), 3), "40 entrées en pages de 15");
        assert_eq!((r.factures.len(), r.au_serveur, r.tronque, l.lignes_ecartees), (n, n as u64, false, 0));
        let journal = faux.journal();
        let appels =
            |o: &str| journal.iter().filter(|x| x.contains(&format!("mcp tools/call {o} (jeton courant)"))).count();
        assert_eq!((appels(compta::OUTIL_DU_REGISTRE), appels(compta::OUTIL_DE_L_HISTORIQUE)), (l.pages, 3));
        assert_eq!(faux.sessions(), 0, "la session est fermée");
        assert!(!journal.join("\n").contains(&jeton), "jamais un jeton au journal du serveur");
        // Chaque facture, champ par champ, telle que la vitrine l'a inventée.
        assert_eq!(r.factures, attendu.factures);
        // Trois d'entre elles, leurs montants calculés à la main : 423,50 $ + 21,18 $ (21,175 : la demie loin de zéro)
        // + 42,24 $ (42,244125) = 486,92 $.
        let tiers = |t: &str| r.factures.iter().find(|f| f.tiers == t).unwrap();
        let f = tiers("Bureau en Gros Fictif");
        assert_eq!(
            (f.cote, f.statut.as_str(), f.projet_id, f.section.clone()),
            (compta::Cote::Fournisseur, "ENVOYEE", None, None)
        );
        assert_eq!(
            (f.sous_total, f.tps, f.tvq, f.total, f.paye, f.solde),
            (Some(42_350), Some(2_118), Some(4_224), Some(48_692), Some(0), Some(48_692))
        );
        assert_eq!(compta::argent(f.total.unwrap()), "486,92\u{a0}$");
        // La note de crédit, stockée positive : 4 500,00 $ + 225,00 $ + 448,88 $ (448,875) = 5 173,88 $.
        let a = r.factures.iter().find(|f| f.est_avoir()).unwrap();
        assert_eq!((a.sous_total, a.tps, a.tvq, a.total), (Some(450_000), Some(22_500), Some(44_888), Some(517_388)));
        // La facture sans total : `null` au serveur, absente ici — jamais zéro.
        let q = tiers("Quincaillerie Modèle");
        assert_eq!((q.sous_total, q.total, q.tps), (None, None, Some(0)));
        // L'historique : le plus récent d'abord (le plus grand id), entier.
        let mut h = attendu.historique.clone();
        h.sort_by(|x, y| y.id.cmp(&x.id));
        assert_eq!(r.historique, h);
        assert!(!r.historique_tronque && !r.historique_absent);

        // registre.json, comme le chef l'écrit ; le fil du centre le relit pour l'onglet.
        let dossier = std::env::temp_dir().join(format!("eve-compta-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dossier);
        fs::create_dir_all(&dossier).unwrap();
        FichierDuRegistre::lu(l.registre.clone(), "2026-11-02T14:05:09").ecrire(&dossier).unwrap();
        let ecrit: compta::Lecture = serde_json::from_slice(&fs::read(dossier.join(compta::FICHIER)).unwrap()).unwrap();
        assert_eq!(
            (ecrit.etat.as_str(), ecrit.lu_le.as_str(), ecrit.erreur.as_str()),
            (A_JOUR, "2026-11-02T14:05:09", "")
        );
        assert_eq!(ecrit.registre.as_ref(), Some(&l.registre));
        let c = crate::centre::Centre::avec_journal(None, Some(dossier.clone()), eframe::egui::Context::default());
        let debut = Instant::now();
        while c.registre().is_none() && debut.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let vu = c.registre().expect("le centre relit registre.json");
        assert_eq!(vu.lu_le, "2026-11-02T14:05:09");
        assert_eq!(vu.registre.as_ref().map(|r| r.factures.len()), Some(n));
        drop(c);
        let _ = fs::remove_dir_all(&dossier);
        let _ = fs::remove_file(&lien.fichier);
    }

    /// Un serveur d'avant la comptabilité (le vrai, tant que l'ERP n'est pas mis à jour) : FastMCP rend « Unknown
    /// tool » en résultat d'erreur, la spécification en erreur JSON-RPC — dans les deux cas, un seul appel, puis la
    /// phrase, mot pour mot ; la session fermée ; et l'entreprise s'y lit comme avant. Un outil de la comptabilité en
    /// panne, un jeton refusé : une lecture manquée, dite.
    #[test]
    fn un_serveur_d_avant_la_compta_le_dit_en_une_phrase() {
        for en_jsonrpc in [false, true] {
            let faux = crate::essais::FauxConstructo::demarrer(0, 3600, 0).unwrap();
            let (jeton, lien) = jeton_du_faux(&faux, if en_jsonrpc { "avant-rpc" } else { "avant" });
            faux.sans_la_compta(en_jsonrpc);
            let p = lire_la_compta(&lien.ressource(), &jeton, PAGES, COMPTA_DUREE_MAX).unwrap_err();
            assert_eq!(p, Panne::Inconnu(compta::OUTIL_DU_REGISTRE.into()), "en JSON-RPC : {en_jsonrpc}");
            assert_eq!(
                phrase_de_la_compta(&p),
                "Constructo AI n'offre pas encore la lecture de la comptabilité : elle viendra avec sa prochaine \
                 mise à jour."
            );
            let journal = faux.journal();
            assert_eq!(journal.iter().filter(|x| x.contains("tools/call comptabilite_")).count(), 1, "{journal:?}");
            assert_eq!(faux.sessions(), 0, "la session est fermée");
            let e = lire(&lien.ressource(), &jeton, &crate::journal::date(), false).unwrap();
            assert!(e.manques.is_empty(), "{:?}", e.manques);
            let _ = fs::remove_file(&lien.fichier);
        }
        // L'historique en panne : le registre lu reste, l'historique dit sa panne (la QA5 du lot 13.2 : la lecture
        // entière manquait). Le registre en panne : la lecture manque.
        let faux = crate::essais::FauxConstructo::demarrer(0, 3600, 0).unwrap();
        let (jeton, lien) = jeton_du_faux(&faux, "historique-en-panne");
        faux.faire_echouer(compta::OUTIL_DE_L_HISTORIQUE);
        let l = lire_la_compta(&lien.ressource(), &jeton, PAGES, COMPTA_DUREE_MAX).unwrap();
        let n = compta::vitrine::registre(&crate::journal::date()).factures.len();
        assert_eq!(
            (l.registre.factures.len(), l.registre.historique.len(), l.registre.historique_tronque),
            (n, 0, true)
        );
        assert_eq!(
            l.registre.historique_erreur,
            "Constructo AI n'a pas pu lire l'historique : « Erreur lors de la lecture (voir logs serveur) »."
        );
        faux.faire_echouer(compta::OUTIL_DU_REGISTRE);
        let p = lire_la_compta(&lien.ressource(), &jeton, PAGES, COMPTA_DUREE_MAX).unwrap_err();
        assert_eq!(
            phrase_de_la_compta(&p),
            "Constructo AI n'a pas pu lire la comptabilité : « Erreur lors de la lecture (voir logs serveur) »."
        );
        let p = lire_la_compta(&lien.ressource(), "eyJ.inconnu.sig", PAGES, COMPTA_DUREE_MAX).unwrap_err();
        assert_eq!(p, Panne::Jeton);
        let _ = fs::remove_file(&lien.fichier);
    }

    /// Un serveur qui refuse l'accès lui-même (un 403, à toute requête) : la lecture de la comptabilité manque d'un
    /// refus ([`Panne::Interdit`] : rien ne reste de l'entreprise, la QA5 du lot 13.2) ; celle de l'entreprise le dit
    /// comme avant.
    #[test]
    fn un_403_refuse_l_acces() {
        use std::io::{BufRead, BufReader, Read, Write};
        let ecoute = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = ecoute.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut flux in ecoute.incoming().flatten() {
                // La demande entière (ses en-têtes, puis son corps), puis le refus.
                let Ok(copie) = flux.try_clone() else { continue };
                let mut lecteur = BufReader::new(copie);
                let (mut ligne, mut longueur) = (String::new(), 0usize);
                loop {
                    ligne.clear();
                    match lecteur.read_line(&mut ligne) {
                        Ok(n) if n > 0 && !ligne.trim().is_empty() => {
                            if let Some((nom, valeur)) = ligne.split_once(':')
                                && nom.trim().eq_ignore_ascii_case("content-length")
                            {
                                longueur = valeur.trim().parse().unwrap_or(0);
                            }
                        }
                        _ => break,
                    }
                }
                let mut corps = vec![0u8; longueur];
                let _ = lecteur.read_exact(&mut corps);
                let _ = flux.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
        let ressource = format!("http://127.0.0.1:{port}/mcp");
        let p = lire_la_compta(&ressource, "jeton", PAGES, Duration::from_secs(10)).unwrap_err();
        assert_eq!(p, Panne::Interdit);
        assert!(refus_d_autorisation(&p));
        assert_eq!(phrase_de_la_compta(&p), "Constructo AI refuse l'accès à ces lectures (HTTP 403).");
        let p = lire(&ressource, "jeton", "2026-11-02", false).unwrap_err();
        assert_eq!(p.texte(), "Constructo AI refuse l'accès à ces lectures (HTTP 403)");
    }

    /// Un connecteur qui ne répond pas, endormi vingt secondes, bien plus que l'échéance de la lecture (1,5 s) : elle
    /// s'arrête à son échéance et le dit, au lieu d'attendre le délai de chaque requête (la QA5 du lot 13.2). Une
    /// marge large : ce qui précède l'envoi (le proxy du poste, la connexion) n'est borné que par attente, et prend
    /// parfois des secondes quand tous les essais tournent ensemble (jusqu'à 6 s mesurées, sous la charge).
    #[test]
    fn une_lecture_trop_longue_s_arrete_sur_le_faux_serveur() {
        let faux = crate::essais::FauxConstructo::demarrer(0, 3600, 0).unwrap();
        let (jeton, lien) = jeton_du_faux(&faux, "trop-longue");
        faux.endormir(20);
        let debut = Instant::now();
        let p = lire_la_compta(&lien.ressource(), &jeton, PAGES, Duration::from_millis(1500)).unwrap_err();
        let duree = debut.elapsed();
        assert_eq!(p, Panne::Serveur(LECTURE_TROP_LONGUE.into()));
        assert!(duree < Duration::from_secs(10), "l'échéance est à 1,5 s, la lecture a pris {duree:?}");
        let _ = fs::remove_file(&lien.fichier);
    }

    /// La mesure d'une grande entreprise : vingt mille factures et mille entrées, `registre.json` écrit comme le chef
    /// l'écrit sur son fil (deux fois par lecture : `en_cours`, puis `a_jour`), et relu comme le fil du centre le
    /// relit. Rien n'y est jugé : `cargo test --release --bin eve mesure_d_un_grand_registre -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn mesure_d_un_grand_registre() {
        let base = compta::vitrine::registre("2026-11-02");
        let factures: Vec<compta::Facture> = base
            .factures
            .iter()
            .cycle()
            .take(compta::FACTURES_MAX)
            .enumerate()
            .map(|(k, f)| compta::Facture { id: k as i64 + 1, ..f.clone() })
            .collect();
        let historique = base.historique.iter().cycle().take(compta::CHANGEMENTS_MAX).cloned().collect();
        let r = compta::Registre { factures, historique, au_serveur: 20_000, ..compta::Registre::default() };
        let dossier = std::env::temp_dir().join(format!("eve-mesure-registre-{}", std::process::id()));
        fs::create_dir_all(&dossier).unwrap();
        let lu = FichierDuRegistre::lu(r, "2026-11-02T14:05:09");
        let t = Instant::now();
        lu.en_cours().ecrire(&dossier).unwrap();
        let en_cours = t.elapsed();
        let t = Instant::now();
        lu.ecrire(&dossier).unwrap();
        let a_jour = t.elapsed();
        let chemin = dossier.join(compta::FICHIER);
        let taille = fs::metadata(&chemin).unwrap().len();
        let t = Instant::now();
        let relu = crate::centre::lire_le_registre(&chemin).unwrap();
        let relire = t.elapsed();
        assert_eq!(relu.registre.map(|r| r.factures.len()), Some(compta::FACTURES_MAX));
        eprintln!(
            "registre.json, 20 000 factures et 1 000 entrées : {taille} octets ; écrit en {en_cours:?} (en cours) et \
             en {a_jour:?} (à jour) ; relu en {relire:?}"
        );
        let _ = fs::remove_dir_all(&dossier);
    }
}

/// QA5 du lot 13.2, passe 2 (mutation) : les essais qui manquaient à la lecture. Chacun passe sur le code d'aujourd'hui
/// et rougit sous le mutant de son nom.
#[cfg(test)]
mod manques_qa2 {
    use super::*;

    /// Une lecture sur un serveur inventé (le registre, puis un historique vide).
    fn lire_le_registre(registre: impl Fn(i64) -> Value) -> LectureDeLaCompta {
        lire_les_pages(
            &mut |outil: &str, a: Value| {
                if outil == compta::OUTIL_DU_REGISTRE {
                    Ok(registre(a["avant_id"].as_i64().unwrap()).to_string())
                } else {
                    Ok(json!({ "changements": [], "suivant": null }).to_string())
                }
            },
            PAGES,
            None,
        )
        .unwrap()
    }

    /// Une dernière page qui dépasse la borne se dit tronquée, même sans `suivant` : vingt et un mille factures
    /// lues, vingt mille gardées (mutant 406 : elle se disait entière).
    #[test]
    fn manque_406_une_derniere_page_qui_depasse_la_borne() {
        let pages_de_3000 = |fin_a_la_septieme: bool| {
            move |avant: i64| {
                let haut = if avant == 0 { 30_001 } else { avant };
                let lignes: Vec<Value> = (1..=3_000).rev().map(|k| json!({ "id": haut - k })).collect();
                let derniere = fin_a_la_septieme && haut == 12_001;
                let suivant = if derniere { Value::Null } else { json!(haut - 3_000) };
                json!({ "factures": lignes, "suivant": suivant, "total": 21_000 })
            }
        };
        let l = lire_le_registre(pages_de_3000(true));
        assert_eq!((l.pages, l.registre.factures.len(), l.registre.tronque), (7, 20_000, true));
    }

    const HEURE: &str = "2026-11-02 14:05:09";

    fn reliee(emis: u64, tour: u64) -> EtatDeLaConnexion {
        EtatDeLaConnexion { reliee: true, occupee: false, acces_valable: true, a_renouveler: false, emis, tour }
    }

    fn une_lecture() -> LectureDeLaCompta {
        let f = compta::Facture { id: 1, ..compta::Facture::default() };
        LectureDeLaCompta {
            registre: compta::Registre { factures: vec![f], ..compta::Registre::default() },
            ..LectureDeLaCompta::default()
        }
    }

    /// Une lecture qui revient quand Eve n'est plus reliée (l'espace de Constructo AI quitté, sans déconnexion : le
    /// même tour) est écartée : rien de l'entreprise dans `registre.json` (mutant 506).
    #[test]
    fn manque_506_une_lecture_revenue_hors_connexion_est_ecartee() {
        let t = Instant::now();
        let c = reliee(1, 1);
        let mut la = LaCompta::default();
        la.demande(false, t, HEURE, &c);
        la.charge(FichierDuRegistre::default(), 1, t, HEURE, &c);
        assert!(la.battement(t, &c).contains(&PourLaCompta::Lire { tour: 1, emis: 1, relire: false }));
        let hors = EtatDeLaConnexion { reliee: false, ..c };
        let ordres = la.lue(1, 1, false, Duration::from_secs(1), Ok(une_lecture()), t, "2026-11-02T14:05:12", &hors);
        assert!(!ordres.iter().any(|o| matches!(o, PourLaCompta::Ecrire(_))), "{ordres:?}");
    }

    /// La connexion perdue pendant que `registre.json` se charge : le battement oublie le chargement et la demande, et
    /// le dit au fichier ; le chargement qui revient ensuite est écarté (mutant 513 : il était pris).
    #[test]
    fn manque_513_la_connexion_perdue_pendant_le_chargement() {
        let t = Instant::now();
        let c = reliee(1, 1);
        let mut la = LaCompta::default();
        assert_eq!(la.demande(true, t, HEURE, &c), vec![PourLaCompta::Charger { tour: 1 }]);
        let hors = EtatDeLaConnexion { reliee: false, ..c };
        assert_eq!(la.battement(t, &hors), vec![PourLaCompta::Ecrire(FichierDuRegistre::non_connecte())]);
        let lu = FichierDuRegistre::lu(compta::Registre::default(), "2026-11-02T14:00:00");
        let ordres = la.charge(lu, 1, t, HEURE, &c);
        assert!(matches!(ordres.as_slice(), [PourLaCompta::Noter(_)]), "{ordres:?}");
        assert_eq!(la.fichier, Some(FichierDuRegistre::non_connecte()));
    }

    /// `registre.json` chargé quand Eve n'est plus reliée : la demande qui l'attendait ne part pas (mutant 518) ; une
    /// demande sans connexion ne charge rien (mutant 521).
    #[test]
    fn manque_518_521_rien_ne_part_hors_connexion() {
        let t = Instant::now();
        let c = reliee(1, 1);
        let hors = EtatDeLaConnexion { reliee: false, ..c };
        assert!(LaCompta::default().demande(true, t, HEURE, &hors).is_empty());
        let mut la = LaCompta::default();
        la.demande(true, t, HEURE, &c);
        la.charge(FichierDuRegistre::default(), 1, t, HEURE, &hors);
        assert!(!la.demandes.voulue());
    }

    /// Un « Relire » qui attend le chargement de `registre.json` le reste, même suivi d'une demande ordinaire (mutant
    /// 519 : la demande ordinaire l'effaçait, et un fichier frais ne se relisait pas).
    #[test]
    fn manque_519_relire_attend_le_chargement() {
        let t = Instant::now();
        let c = reliee(1, 1);
        let mut la = LaCompta::default();
        assert_eq!(la.demande(true, t, HEURE, &c), vec![PourLaCompta::Charger { tour: 1 }]);
        assert!(la.demande(false, t, HEURE, &c).is_empty());
        let frais = FichierDuRegistre::lu(compta::Registre::default(), "2026-11-02T14:04:00");
        la.charge(frais, 1, t, HEURE, &c);
        assert!(la.battement(t, &c).contains(&PourLaCompta::Lire { tour: 1, emis: 1, relire: true }));
    }

    /// Un chargement demandé sous un autre tour de connexion est écarté, même sans déconnexion vue entre-temps (mutant
    /// 516).
    #[test]
    fn manque_516_un_chargement_d_un_autre_tour() {
        let t = Instant::now();
        let mut la = LaCompta::default();
        la.demande(false, t, HEURE, &reliee(1, 1));
        let ordres = la.charge(FichierDuRegistre::default(), 1, t, HEURE, &reliee(1, 2));
        assert!(matches!(ordres.as_slice(), [PourLaCompta::Noter(_)]), "{ordres:?}");
        assert!(la.fichier.is_none());
    }
}
