//! Le tableau de bord de l'entreprise : Eve lit elle-même Constructo AI, l'ERP de l'entreprise, pour la montrer d'un
//! coup d'œil — ses chiffres, ses alertes, ses chantiers en cours, ses factures en retard, sa balance âgée, son
//! pipeline, ses soumissions, les factures de ses fournisseurs, ses achats en route, les heures de ses employés,
//! l'agenda partagé, ses comptes en banque. Elle le fait en cliente MCP, comme Claude Code, mais en lecture seule :
//! elle n'appelle que les outils nommés un à un dans [`OUTILS_LUS`], que la garde tient aussi pour des lectures
//! (`garde::est_lecture_constructo`) — jamais un outil d'écriture, même par erreur.
//!
//! Deux cartes du tableau d'Entrepreneur Général n'ont pas d'outil de lecture ici : les demandes de prix (le module
//! des appels d'offres de l'ERP n'a pas d'outil MCP), et les tâches du calendrier de production
//! (`suivi_lister_operations` pose treize `ALTER TABLE` à chaque appel : trop lourd pour une lecture aux dix minutes).
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
const AGENDA_MAX: usize = 30;
/// L'agenda : aujourd'hui et les jours qui suivent.
const AGENDA_JOURS: i64 = 14;
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
/// une lecture.
pub const OUTILS_LUS: [&str; 14] = [
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
];

/// Un outil qu'Eve peut appeler elle-même : nommé dans [`OUTILS_LUS`] et tenu pour une lecture par la garde — les
/// deux, pour qu'une erreur d'un côté ne laisse jamais passer une écriture.
pub fn permis(outil: &str) -> bool {
    OUTILS_LUS.contains(&outil) && est_lecture_constructo(outil)
}

/// Les appels d'une lecture, de l'essentiel au reste : un serveur qui flanche en route a déjà rendu les chiffres.
/// Les pointages, en deux appels (cette semaine, la précédente) : l'outil en rend 200 au plus.
fn appels(aujourdhui: &str) -> Vec<(&'static str, Value)> {
    let jour = |decalage: i64| numero_du_jour(aujourdhui).map(|n| date_du_numero(n + decalage)).unwrap_or_default();
    // Le lundi de cette semaine, en jours depuis aujourd'hui (le 1er janvier 1970, jour zéro, était un jeudi).
    let lundi = numero_du_jour(aujourdhui).map_or(0, |n| -(n + 3).rem_euclid(7));
    vec![
        ("tableau_kpis", json!({ "periode_jours": 30 })),
        ("tableau_alertes", json!({})),
        ("tableau_factures_agees", json!({ "inclure_detail": true, "limite": FACTURES_MAX })),
        ("lister_projets", json!({ "statut": "En cours", "limite": 50 })),
        ("tableau_pipeline_ventes", json!({})),
        // Les 50 devis les plus récents : les soumissions en cours, et ceux qui attendent une réponse.
        ("lister_devis", json!({ "limite": 50 })),
        ("lister_pointages", json!({ "date_debut": jour(lundi), "date_fin": jour(lundi + 6), "limite": 200 })),
        ("lister_pointages", json!({ "date_debut": jour(lundi - 7), "date_fin": jour(lundi - 1), "limite": 200 })),
        (
            "suivi_lister_notes_calendrier",
            json!({ "date_debut": aujourdhui, "date_fin": jour(AGENDA_JOURS), "limite": 50 }),
        ),
        // Les factures de l'ERP mêlent clients et fournisseurs : celles des fournisseurs se trient ici.
        ("lister_factures", json!({ "limite": 100 })),
        ("lister_bons_commande", json!({ "limite": 30 })),
        ("lister_bons_achat", json!({ "limite": 30 })),
        ("lister_livraisons", json!({ "statut": "planifiee", "limite": 30 })),
        ("banque_lister_comptes", json!({})),
        ("conformite_expirations", json!({ "jours": CONFORMITE_JOURS })),
    ]
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

/// Un projet en cours ; ses dates (AAAA-MM-JJ) sont celles de son début et de sa fin dans l'ERP. Aucun outil ne rend
/// l'avancement d'une liste de projets : l'ERP le calcule de leurs phases.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Projet {
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
/// dernière modification. L'ERP ne lui garde pas d'adresse : elle est d'ordinaire dans son nom.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Devis {
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

/// Un achat en route : `sorte` — `a_commander` (un bon d'achat pas encore commandé ; `date` : son échéance),
/// `commande` (un bon de commande pas encore reçu ; `date` : la livraison prévue), `livraison` (une livraison de
/// chantier prévue ; `date` et `heure`). `texte` : ses notes, courtes (d'une livraison : sa sorte et sa zone).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Commande {
    pub sorte: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub numero: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fournisseur: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statut: Option<String>,
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
/// outils qui n'ont rien rendu de lisible, et ses temps.
#[derive(Clone, Debug)]
pub struct Lecture {
    pub entreprise: Entreprise,
    pub manques: Vec<String>,
    /// L'ouverture de la session (le réveil du serveur, s'il dormait), et toute la lecture.
    pub ouverture: Duration,
    pub duree: Duration,
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
        format!(
            "{} outils lus sur {} en {} (la session ouverte en {}) : {} projets en cours, {} factures en retard, {} \
             alertes, {} notes d'agenda, {} soumissions, {} devis en attente, {} factures reçues, {} achats en route, \
             {employes} employés pointés, {} comptes bancaires{sans}",
            OUTILS_LUS.len() - self.manques.len(),
            OUTILS_LUS.len(),
            secondes(self.duree),
            secondes(self.ouverture),
            n(&e.projets),
            n(&e.factures_en_retard),
            n(&e.alertes),
            n(&e.agenda),
            n(&e.soumissions),
            n(&e.devis),
            n(&e.factures_recues),
            n(&e.commandes),
            n(&e.banque),
        )
    }
}

/// « 3,2 s ».
pub fn secondes(d: Duration) -> String {
    format!("{:.1} s", d.as_secs_f64()).replace('.', ",")
}

/// Une lecture dans un fil (elle parle au serveur, qui dort parfois) : `rapporter` reçoit son temps et ce qu'elle a
/// donné ; une panique y devient une panne.
pub fn lire_dans_un_fil(
    ressource: String,
    jeton: String,
    aujourdhui: String,
    rapporter: impl FnOnce(Duration, Result<Lecture, Panne>) + Send + 'static,
) {
    std::thread::spawn(move || {
        let debut = Instant::now();
        let issue = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lire(&ressource, &jeton, &aujourdhui)))
            .unwrap_or_else(|_| Err(Panne::Serveur("la lecture de Constructo AI a planté (voir eve.log)".into())));
        rapporter(debut.elapsed(), issue);
    });
}

/// Une lecture de l'ERP : une session MCP au connecteur (`ressource` : `<serveur>/mcp`), un appel par outil, puis la
/// session fermée. Un outil qui flanche manque à la lecture ; un serveur qui flanche l'arrête ; aucun outil lisible :
/// une panne.
pub fn lire(ressource: &str, jeton: &str, aujourdhui: &str) -> Result<Lecture, Panne> {
    let debut = Instant::now();
    let mut client = Client::ouvrir(ressource, jeton)?;
    let ouverture = debut.elapsed();
    let mut reponses: Vec<(&str, Result<String, String>)> = Vec::new();
    for (outil, arguments) in appels(aujourdhui) {
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
    if manques.len() == OUTILS_LUS.len() {
        let raison = reponses.iter().find_map(|(_, r)| r.as_ref().err()).cloned().unwrap_or_default();
        return Err(Panne::Serveur(format!("Constructo AI n'a rien rendu de lisible ({})", ecourter(&raison, 160))));
    }
    Ok(Lecture { entreprise, manques, ouverture, duree: debut.elapsed() })
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

    // Les projets en cours, et leur nom par leur numéro dans l'ERP : les pointages, les factures et les livraisons ne
    // donnent que celui-ci.
    let mut noms_des_projets: HashMap<u64, (String, Option<String>)> = HashMap::new();
    match un("lister_projets").and_then(liste) {
        Some(l) => {
            let mut projets = Vec::new();
            for p in &l {
                let Some(nom) = texte(&p["nom_projet"]) else { continue };
                let numero = texte(&p["numero_projet"]);
                if let Some(id) = entier(&p["id"]) {
                    noms_des_projets.insert(id, (nom.clone(), numero.clone()));
                }
                projets.push(Projet {
                    nom,
                    numero,
                    client: texte(&p["client_nom_cache"]).or_else(|| texte(&p["client_nom_direct"])),
                    statut: texte(&p["statut"]),
                    // `budget_total` : la colonne canonique de l'ERP ; `prix_estime`, l'ancienne.
                    valeur: positif(&p["budget_total"]).or_else(|| positif(&p["prix_estime"])),
                    ville: texte(&p["ville_chantier"]),
                    debut: date(&p["date_debut_reel"]),
                    fin: date(&p["date_fin_reel"]),
                    adresse: texte(&p["adresse_chantier"]),
                });
            }
            e.kpis.valeur_projets_en_cours = Some(projets.iter().filter_map(|p| p.valeur).sum());
            e.projets = Some(projets.into_iter().take(PROJETS_MAX).collect());
        }
        None => noter(&mut manques, "lister_projets"),
    }
    let nom_du_projet = |v: &Value| entier(v).and_then(|id| noms_des_projets.get(&id)).map(|(nom, _)| nom.clone());

    match un("tableau_pipeline_ventes").and_then(objet).filter(|v| v["par_statut"].is_array()) {
        Some(v) => {
            let ferme = |s: &str| matches!(normaliser(s).as_str(), "gagne" | "gagnee" | "perdu" | "perdue");
            e.pipeline = Some(
                v["par_statut"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| {
                        let etape = texte(&p["statut"])?;
                        let valeur = montant(&p["valeur"]).unwrap_or(0.0);
                        (!ferme(&etape)).then(|| Etape { etape, nombre: entier(&p["nb"]).unwrap_or(0), valeur })
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
                        numero: texte(&d["numero_devis"]),
                        nom: texte(&d["nom_projet"])?,
                        client: texte(&d["client_nom_cache"]).or_else(|| texte(&d["client_nom_direct"])),
                        statut: texte(&d["statut"]).unwrap_or_default(),
                        // `investissement_total` : le montant que somme l'ERP pour ses devis.
                        valeur: positif(&d["investissement_total"]).or_else(|| positif(&d["prix_estime"])),
                        depuis: date(&d["date_soumis"]).or_else(|| date(&d["created_at"])),
                        modifie: date(&d["updated_at"]).or_else(|| date(&d["created_at"])),
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
    match un("lister_factures").and_then(liste) {
        Some(l) => {
            let recues = l
                .iter()
                .filter(|f| f["type_destinataire"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("fournisseur")))
                .map(|f| FactureRecue {
                    numero: texte(&f["numero_facture"]),
                    fournisseur: texte(&f["client_nom"]),
                    projet: nom_du_projet(&f["project_id"]),
                    date: date(&f["date_facture"]),
                    montant: montant(&f["montant_ttc"]).or_else(|| montant(&f["montant_total"])),
                    solde: montant(&f["solde_du"]),
                    statut: texte(&f["statut"]),
                });
            e.factures_recues = Some(recues.take(FACTURES_RECUES_MAX).collect());
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
                    numero: texte(&b["numero_bon"]).or_else(|| texte(&b["numero"])),
                    fournisseur: texte(&b["fournisseur_nom"]),
                    projet: texte(&b["projet_nom"]),
                    statut: texte(&b["statut"]),
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

    match un("suivi_lister_notes_calendrier").and_then(liste) {
        Some(l) => {
            let fin = numero_du_jour(aujourdhui).map(|n| date_du_numero(n + AGENDA_JOURS)).unwrap_or_default();
            let mut notes: Vec<Note> = l
                .iter()
                .filter_map(|n| Some(Note { quand: date(&n["note_date"])?, texte: texte(&n["contenu"])? }))
                .filter(|n| n.quand.as_str() >= aujourdhui && (fin.is_empty() || n.quand <= fin))
                .collect();
            notes.sort_by(|a, b| a.quand.cmp(&b.quand));
            notes.truncate(AGENDA_MAX);
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
    (e, manques)
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

    /// Les factures, clients et fournisseurs mêlées (`type_destinataire`, absent d'une facture d'avant sa colonne).
    const FACTURES: &str = r#"4 facture(s) trouvée(s):
[
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

    /// Toutes les réponses d'une lecture, telles que le vrai serveur les rendrait, dans l'ordre des appels.
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
        // L'agenda : d'aujourd'hui aux 14 jours suivants, dans l'ordre ; ni le nom de qui l'a écrite.
        let a = e.agenda.as_ref().unwrap();
        assert_eq!(a.iter().map(|n| n.quand.as_str()).collect::<Vec<_>>(), ["2026-10-27", "2026-11-02"]);
        assert_eq!(a[0].texte, "Visite de chantier, 12 rue des Exemples");
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
        for (o, _) in appels("2026-10-26") {
            assert!(OUTILS_LUS.contains(&o), "{o}");
        }
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
        let lecture = Lecture { entreprise: lu, manques: Vec::new(), ouverture: Duration::ZERO, duree: Duration::ZERO };
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
        let e = Entreprise::lue(
            &Lecture { entreprise: lu, manques, ouverture: Duration::ZERO, duree: Duration::ZERO },
            "2026-10-26T13:45:00-04:00",
        );
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
        let l = lire(&ressource, &jeton, &aujourdhui).unwrap();
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
            ["Réunion de chantier, 8 h", "Visite de chantier, 12 rue des Exemples", "Livraison des fermes de toit"]
        );
        assert_eq!((e.banque.as_ref().map(Vec::len), k.solde_banque), (Some(2), Some(52010.43)));
        assert_eq!(e.alertes.as_ref().map(Vec::len), Some(5));
        assert_eq!(faux.sessions(), 0, "la session est fermée");
        assert!(l.resume().starts_with("14 outils lus sur 14 en "), "{}", l.resume());
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
        let l = lire(&ressource, &jeton, &aujourdhui).unwrap();
        assert_eq!(l.manques, ["lister_projets"]);
        assert!(l.entreprise.projets.is_none() && l.entreprise.kpis.projets_en_cours == Some(3));
        assert_eq!(faux.journal().iter().filter(|x| x.ends_with("/mcp renvoyé vers /mcp/")).count(), 1);
        assert_eq!(faux.sessions(), 0);
        // Un jeton que le serveur ne connaît pas : refusé, rien de lu.
        assert_eq!(lire(&ressource, "eyJ.inconnu.sig", &aujourdhui).unwrap_err(), Panne::Jeton);
        // Un serveur absent : une panne, qui le dit.
        match lire("http://127.0.0.1:9/mcp", &jeton, &aujourdhui) {
            Err(Panne::Serveur(m)) => assert!(m.starts_with("Constructo AI ne répond pas"), "{m}"),
            autre => panic!("{autre:?}"),
        }
        let _ = fs::remove_file(&lien.fichier);
    }
}
