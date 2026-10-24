//! La mémoire d'Eve, d'une séance à l'autre : les derniers échanges avec chaque personne (sa demande, la réponse
//! d'Eve, écourtées), dans `%LOCALAPPDATA%\EntrepreneurGeneral\Eve\memoire\<Prénom>.json` — sur le poste seulement.
//! Au démarrage, ils sont donnés à l'Adjointe (« et pour vendredi ? » garde son sens le lendemain) et les trois
//! derniers s'affichent, plus pâles, en tête du fil. « Nouvelle conversation » les oublie ; la copie d'avant reste à
//! côté (`<Prénom>.avant.json`), le temps de changer d'avis.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::reglages::{dossier_local, ecrire_d_un_geste};

/// Les échanges gardés par personne.
const GARDES: usize = 30;
/// Les échanges donnés à l'Adjointe au démarrage.
const DONNES: usize = 12;

/// Un échange : la demande de la personne et la réponse d'Eve.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Echange {
    pub date: String,
    pub heure: String,
    pub demande: String,
    pub reponse: String,
}

fn fichier(qui: &str, suffixe: &str) -> PathBuf {
    let nom: String = qui.chars().filter(|c| c.is_alphanumeric() || *c == '-').collect();
    let nom = if nom.is_empty() { "poste".to_string() } else { nom };
    let dossier = dossier_local().join("memoire");
    let _ = fs::create_dir_all(&dossier);
    dossier.join(format!("{nom}{suffixe}.json"))
}

/// Les échanges gardés avec cette personne, du plus ancien au plus récent.
pub fn charger(qui: &str) -> Vec<Echange> {
    fs::read_to_string(fichier(qui, "")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Garde un échange de plus (les plus anciens s'effacent au-delà de trente) : le fichier se réécrit d'un seul geste,
/// et une coupure en route le laisse comme il était.
pub fn ajouter(qui: &str, echange: Echange) {
    let mut l = charger(qui);
    l.push(echange);
    if l.len() > GARDES {
        let trop = l.len() - GARDES;
        l.drain(..trop);
    }
    if let Ok(t) = serde_json::to_string_pretty(&l) {
        let _ = ecrire_d_un_geste(&fichier(qui, ""), t.as_bytes());
    }
}

/// Oublie les échanges avec cette personne (la copie d'avant reste à côté).
pub fn oublier(qui: &str) {
    let f = fichier(qui, "");
    if f.is_file() {
        let _ = fs::rename(&f, fichier(qui, ".avant"));
    }
}

/// Le bloc de mémoire donné à l'Adjointe : les derniers échanges, du plus ancien au plus récent.
pub fn pour_l_adjointe(qui: &str, echanges: &[Echange]) -> String {
    let mut s = format!(
        "Mémoire d'Eve — nos derniers échanges avec {qui}, d'une séance précédente, du plus ancien au plus récent \
         (pour comprendre une suite ; ne les récite pas) :"
    );
    let debut = echanges.len().saturating_sub(DONNES);
    for e in &echanges[debut..] {
        s.push_str(&format!("\n- {} {} — {qui} : « {} » → Eve : « {} »", e.date, e.heure, e.demande, e.reponse));
    }
    s
}
