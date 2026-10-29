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

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eve::garde::{ecourter, est_lecture_constructo};
use eve::parole::normaliser;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::centre::{date_du_numero, jours_entre, milliers, numero_du_jour};
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
/// une lecture. Les derniers, ceux du suivi de production ([`OUTILS_DU_SUIVI`]), seulement à la demande.
pub const OUTILS_LUS: [&str; 22] = [
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
];

/// Le suivi de production : les opérations des bons de travail (leur responsable nommé d'après la liste des employés),
/// les dépendances et les références du Gantt. Ils ne se lisent qu'à la demande ([`SUIVI_INTERVALLE`]).
pub const OUTILS_DU_SUIVI: [&str; 4] =
    ["suivi_lister_operations", "suivi_lister_dependances_gantt", "suivi_lister_baselines", "lister_employes"];

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
        }
    }
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
    let mut client = Client::ouvrir(ressource, jeton)?;
    let ouverture = debut.elapsed();
    let appels = appels(aujourdhui, suivi);
    let outils = outils_de(&appels).len();
    let mut reponses: Vec<(&str, Result<String, String>)> = Vec::new();
    for (outil, arguments) in appels {
        match client.appeler(outil, arguments) {
            Ok(texte) => reponses.push((outil, Ok(texte))),
            Err(p @ (Panne::Outil(_) | Panne::Refuse(_))) => reponses.push((outil, Err(p.texte()))),
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
}

impl Client {
    fn ouvrir(url: &str, jeton: &str) -> Result<Client, Panne> {
        let mut c = Client {
            url: url.trim_end_matches('/').to_string(),
            jeton: jeton.to_string(),
            session: None,
            version: None,
            suivant: 1,
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
        for _ in 0..2 {
            let entetes = self.entetes();
            let entetes: Vec<(&str, &str)> = entetes.iter().map(|(n, v)| (*n, v.as_str())).collect();
            let r = https::requete("POST", &self.url, &self.jeton, &entetes, Some(("application/json", &corps)), delai)
                .map_err(|e| Panne::Serveur(format!("Constructo AI ne répond pas : {e}")))?;
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
            return Err(Panne::Outil(format!("{outil} : {}", erreur_jsonrpc(e))));
        }
        let resultat = &message["result"];
        let texte = texte_du_resultat(resultat);
        if resultat["isError"].as_bool() == Some(true) {
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
        assert_eq!((sans_suivi.len(), avec_suivi.len()), (18, OUTILS_LUS.len()));
        for o in OUTILS_DU_SUIVI {
            assert!(!sans_suivi.contains(&o) && avec_suivi.contains(&o), "{o}");
        }
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
}
