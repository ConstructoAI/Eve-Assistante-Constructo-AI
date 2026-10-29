//! Le planning : ce que montrent le calendrier, le Gantt et le Kanban du tableau de bord (le thème « Constructo AI ») —
//! l'entreprise, lue dans Constructo AI en lecture seule (`entreprise.rs`, `entreprise.json`), et la maison (ses
//! échéances, ses rappels, ses anniversaires, son école : `activite\*.js`, `rappels.json`). Une seule forme pour les
//! trois vues : les données la remplissent (`Centre`, d'après ses fichiers), les vues la dessinent (`natif\`).
//!
//! C'est le contrat de l'équipe du planning (le lot 12) : ses types ne changent qu'ensemble, d'un commun accord. Une
//! date s'écrit « AAAA-MM-JJ », une heure « HH:MM » ; chaque chose garde sa `source`, ce qu'Eve en dit à Claude quand
//! la personne lui en parle (« En parler à Eve ») — une demande, jamais une écriture : déplacer une carte ou une tâche
//! passe par Claude, et par l'accord de la personne là où les accords valent.

// Le contrat, avant ses deux moitiés : l'équipe du planning le remplit et le dessine.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// D'où vient une chose : le filtre « Tout · Entreprise · Famille » du tableau de bord (`vue_du_tableau`) vaut pour
/// le planning aussi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cote {
    Entreprise,
    Famille,
}

/// Ce qu'est un événement du calendrier : sa sorte lui donne sa teinte et son icône.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sorte {
    /// Un rendez-vous (l'agenda de l'ERP, l'agenda de la maison).
    RendezVous,
    /// Le début ou la fin d'un chantier.
    Chantier,
    /// Une opération d'un chantier (un bon de travail, une tâche du suivi de production).
    Operation,
    /// Une note du calendrier de l'ERP.
    Note,
    /// Une échéance (fiscale, de l'école, d'un document).
    Echeance,
    /// Une facture qui vient à échéance, ou qui l'a passée.
    Facture,
    /// Une livraison ou une commande attendue.
    Livraison,
    /// Un rappel d'Eve (`rappels.json`).
    Rappel,
    Anniversaire,
    /// L'école : un congé, une journée pédagogique.
    Ecole,
    Autre,
}

/// Un événement du calendrier : un jour (ou plusieurs, `fin` comprise), peut-être une heure.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evenement {
    pub debut: String,
    pub fin: Option<String>,
    pub heure: Option<String>,
    pub titre: String,
    pub detail: String,
    pub sorte: Sorte,
    pub cote: Cote,
    /// Ce qu'Eve en dit à Claude quand on lui en parle (« la facture F-1042 de Toitures Exemple, échue le 2 novembre »).
    pub source: String,
}

/// Une tâche du Gantt : un chantier (sans parent) ou l'une de ses opérations (son chantier pour parent).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tache {
    pub id: String,
    pub parent: Option<String>,
    pub titre: String,
    pub debut: String,
    pub fin: String,
    /// De 0 à 1, si l'ERP le dit.
    pub avancement: Option<f32>,
    /// Le statut de l'ERP, tel quel (« En cours », « Termine »…).
    pub statut: String,
    /// Les dates de référence (la « baseline » de l'ERP), pour voir un retard.
    pub prevu: Option<(String, String)>,
    pub responsables: Vec<String>,
    pub source: String,
}

/// Une dépendance entre deux tâches du Gantt : `vers` attend `de` (« FS » : fin à début ; « SS », « FF », « SF »), de
/// tant de jours (`decalage`, négatif : en avance).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Dependance {
    pub de: String,
    pub vers: String,
    pub sorte: String,
    pub decalage: i32,
}

/// Un tableau du Kanban : un « board » du suivi de l'ERP (les chantiers, les soumissions, les bons de travail, les
/// ventes…), ses colonnes dans l'ordre de l'ERP (`suivi_colonnes`), et sa règle (« Statut géré via le Bon de travail »).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tableau {
    pub cle: String,
    pub titre: String,
    pub colonnes: Vec<Colonne>,
    pub garde: Option<String>,
}

/// Une colonne du Kanban : son statut (celui de l'ERP), son titre lisible, combien de cartes l'ERP y compte (`total`,
/// d'après `suivi_etat_kanban`), et celles qu'Eve en a lues.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Colonne {
    pub statut: String,
    pub titre: String,
    pub total: usize,
    pub cartes: Vec<Carte>,
}

/// Une carte du Kanban : un chantier, une soumission, un bon de travail…
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Carte {
    pub id: String,
    pub titre: String,
    pub detail: String,
    pub montant: Option<f64>,
    pub echeance: Option<String>,
    pub responsables: Vec<String>,
    pub source: String,
}

/// Le planning entier, d'un coup : ce que dessinent les trois vues.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Planning {
    pub evenements: Vec<Evenement>,
    pub taches: Vec<Tache>,
    pub dependances: Vec<Dependance>,
    pub tableaux: Vec<Tableau>,
    /// Quand l'entreprise en a été lue (« 2026-10-28 14:02 ») ; vide : pas encore (ou pas reliée).
    pub lu: String,
}
