//! L'historique des conversations d'Eve : chaque
//! échange — la demande, la réponse entière, l'heure, les pièces jointes — gardé sur le poste, par personne et par
//! mois, dans `%LOCALAPPDATA%\EntrepreneurGeneral\Eve\historique\<Prénom>\AAAA-MM.jsonl` : hors du dossier partagé, comme la
//! mémoire (ni montants ni heures de chantier dans le dossier partagé).
//!
//! Une conversation va d'un démarrage d'Eve, ou d'une « Nouvelle conversation », à la suivante ; son identifiant est
//! l'heure où elle a commencé (« 2026-10-22 09-14-03 »). La fenêtre les relit (`GET /historique`), y cherche, en ouvre
//! une (`GET /historique/conversation`) et la reprend : l'Adjointe reçoit alors ses échanges avec la demande suivante.
//!
//! La mémoire (`memoire.rs`) reste à part : ce sont les derniers échanges, écourtés, donnés d'office au démarrage ;
//! l'historique, lui, garde tout, et ne se donne que sur demande.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use eve::garde;
use eve::parole::normaliser;
use serde::{Deserialize, Serialize};

use crate::reglages::dossier_local;

/// Une demande ou une réponse plus longue est écourtée dans l'historique.
const TEXTE_MAX: usize = 20_000;
/// Les échanges d'une conversation reprise donnés à l'Adjointe : les plus récents.
const DONNES: usize = 15;

/// Un échange, une ligne du fichier du mois.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entree {
    pub conversation: String,
    pub date: String,
    pub heure: String,
    pub demande: String,
    pub reponse: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pieces: Vec<String>,
    /// « interrompue » ou « erreur » ; vide : une réponse complète.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub issue: String,
}

/// Une conversation, pour la liste.
#[derive(Debug, Serialize)]
pub struct Resume {
    pub id: String,
    /// Le jour et l'heure du premier échange, et l'heure du dernier (qui peut être d'un autre jour : une conversation
    /// reprise).
    pub date: String,
    pub debut: String,
    pub fin_date: String,
    pub fin: String,
    pub echanges: usize,
    /// La première demande, écourtée.
    pub titre: String,
    /// Un bout du texte autour de ce qu'on cherche.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extrait: Option<String>,
}

/// Le nom du dossier d'une personne : ses lettres, ses chiffres et ses tirets seulement.
fn nom_sur(qui: &str) -> String {
    let nom: String = qui.chars().filter(|c| c.is_alphanumeric() || *c == '-').take(40).collect();
    if nom.is_empty() { "poste".into() } else { nom }
}

fn dossier(qui: &str) -> PathBuf {
    dossier_dans(&dossier_local(), qui)
}

/// Le dossier d'une personne sous un dossier local : celui d'Eve, ou celui d'un essai, qui n'a ainsi jamais à lire
/// l'historique d'une vraie Eve.
pub fn dossier_dans(local: &Path, qui: &str) -> PathBuf {
    local.join("historique").join(nom_sur(qui))
}

/// Garde un échange de plus, au bout du fichier du mois : un ajout, jamais une réécriture — une coupure en route ne
/// touche pas aux échanges d'avant. La ligne part d'une seule écriture, sa fin comprise : sans sa fin, la suivante s'y
/// collerait, et les deux se perdraient à la lecture.
pub fn ajouter(qui: &str, entree: &Entree) -> io::Result<()> {
    let d = dossier(qui);
    fs::create_dir_all(&d)?;
    let mois =
        entree.date.get(..7).filter(|m| m.bytes().all(|b| b.is_ascii_digit() || b == b'-')).unwrap_or("sans-date");
    let mut e = entree.clone();
    e.demande = garde::ecourter(&e.demande, TEXTE_MAX);
    e.reponse = garde::ecourter(&e.reponse, TEXTE_MAX);
    let ligne = serde_json::to_string(&e).map_err(io::Error::other)?;
    let mut f = OpenOptions::new().create(true).append(true).open(d.join(format!("{mois}.jsonl")))?;
    f.write_all(format!("{ligne}\n").as_bytes())
}

/// Tous les échanges gardés dans le dossier d'une personne, du plus ancien au plus récent (une ligne illisible est
/// sautée).
fn toutes(dossier: &Path) -> Vec<Entree> {
    let Ok(lecture) = fs::read_dir(dossier) else { return Vec::new() };
    let mut fichiers: Vec<PathBuf> =
        lecture.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "jsonl")).collect();
    fichiers.sort();
    fichiers
        .iter()
        .filter_map(|f| fs::read_to_string(f).ok())
        .flat_map(|t| t.lines().filter_map(|l| serde_json::from_str::<Entree>(l).ok()).collect::<Vec<_>>())
        .collect()
}

/// Les conversations avec cette personne, la plus récente d'abord ; avec `recherche`, celles où chacun de ses mots
/// paraît (sans égard aux accents ni aux majuscules ; le dernier mot peut n'être qu'un début : « fact » trouve
/// « facture »).
pub fn conversations(qui: &str, recherche: &str, limite: usize) -> Vec<Resume> {
    conversations_dans(&dossier_local(), qui, recherche, limite)
}

/// [`conversations`], sous un autre dossier local que celui d'Eve ([`dossier_dans`]).
pub fn conversations_dans(local: &Path, qui: &str, recherche: &str, limite: usize) -> Vec<Resume> {
    regrouper(toutes(&dossier_dans(local, qui)), recherche, limite)
}

fn regrouper(entrees: Vec<Entree>, recherche: &str, limite: usize) -> Vec<Resume> {
    let mots: Vec<String> = normaliser(recherche).split_whitespace().map(str::to_string).collect();
    let mut ordre: Vec<String> = Vec::new();
    let mut groupes: HashMap<String, Vec<Entree>> = HashMap::new();
    for e in entrees {
        if !groupes.contains_key(&e.conversation) {
            ordre.push(e.conversation.clone());
        }
        groupes.entry(e.conversation.clone()).or_default().push(e);
    }
    let mut resumes: Vec<Resume> = ordre
        .into_iter()
        .filter_map(|id| {
            let g = groupes.remove(&id)?;
            let extrait = if mots.is_empty() {
                None
            } else {
                let tout: Vec<String> =
                    g.iter().flat_map(|e| [normaliser(&e.demande), normaliser(&e.reponse)]).collect();
                if !mots.iter().enumerate().all(|(i, m)| tout.iter().any(|t| contient_mot(t, m, i + 1 == mots.len()))) {
                    return None;
                }
                g.iter().find_map(|e| extrait(&e.demande, &mots).or_else(|| extrait(&e.reponse, &mots)))
            };
            let (premier, dernier) = (g.first()?, g.last()?);
            Some(Resume {
                id,
                date: premier.date.clone(),
                debut: premier.heure.clone(),
                fin_date: dernier.date.clone(),
                fin: dernier.heure.clone(),
                echanges: g.len(),
                titre: garde::ecourter(premier.demande.trim(), 90),
                extrait,
            })
        })
        .collect();
    // La plus récente d'abord : celle dont le dernier échange est le plus récent.
    resumes.sort_by(|a, b| (&b.fin_date, &b.fin, &b.id).cmp(&(&a.fin_date, &a.fin, &a.id)));
    resumes.truncate(limite);
    resumes
}

/// Le texte (déjà normalisé) contient-il ce mot ? En entier, ou en début de mot pour le dernier mot cherché.
fn contient_mot(texte: &str, mot: &str, debut_permis: bool) -> bool {
    texte.split_whitespace().any(|t| t == mot || (debut_permis && t.starts_with(mot)))
}

/// Une quinzaine de mots du texte, tels qu'ils ont été écrits, autour du premier mot cherché.
fn extrait(texte: &str, mots: &[String]) -> Option<String> {
    let premier = mots.first()?;
    let dernier_seul = mots.len() == 1;
    let originaux: Vec<&str> = texte.split_whitespace().collect();
    let i = originaux.iter().position(|o| {
        normaliser(o).split_whitespace().any(|t| t == premier || (dernier_seul && t.starts_with(premier.as_str())))
    })?;
    let (debut, fin) = (i.saturating_sub(6), (i + 10).min(originaux.len()));
    let mut s = originaux[debut..fin].join(" ");
    if debut > 0 {
        s.insert_str(0, "… ");
    }
    if fin < originaux.len() {
        s.push_str(" …");
    }
    Some(garde::ecourter(&s, 160))
}

/// Les échanges d'une conversation, du plus ancien au plus récent.
pub fn conversation(qui: &str, id: &str) -> Vec<Entree> {
    conversation_dans(&dossier_local(), qui, id)
}

/// [`conversation`], sous un autre dossier local que celui d'Eve ([`dossier_dans`]).
pub fn conversation_dans(local: &Path, qui: &str, id: &str) -> Vec<Entree> {
    toutes(&dossier_dans(local, qui)).into_iter().filter(|e| e.conversation == id).collect()
}

/// Le bloc donné à l'Adjointe quand la personne reprend une conversation : ses derniers échanges, écourtés.
pub fn pour_l_adjointe(qui: &str, entrees: &[Entree]) -> String {
    let date = entrees.first().map(|e| e.date.as_str()).unwrap_or("");
    let mut s = format!(
        "[Conversation du {date}, reprise par {qui} dans l'historique d'Eve — nos échanges, du plus ancien au plus récent \
         (pour comprendre la suite ; ne les récite pas) :"
    );
    let debut = entrees.len().saturating_sub(DONNES);
    for e in &entrees[debut..] {
        s.push_str(&format!(
            "\n- {} {} — {qui} : « {} » → Eve : « {} »",
            e.date,
            e.heure,
            garde::ecourter(e.demande.trim(), 300),
            garde::ecourter(e.reponse.trim(), 700)
        ));
    }
    s.push(']');
    s
}

#[cfg(test)]
mod essais {
    use super::*;

    fn entree(conversation: &str, date: &str, heure: &str, demande: &str, reponse: &str) -> Entree {
        Entree {
            conversation: conversation.into(),
            date: date.into(),
            heure: heure.into(),
            demande: demande.into(),
            reponse: reponse.into(),
            pieces: Vec::new(),
            issue: String::new(),
        }
    }

    fn exemple() -> Vec<Entree> {
        vec![
            entree(
                "2026-10-21 08-00-00", "2026-10-21", "08:01", "Qu'est-ce que j'ai demain ?",
                "Trois choses au calendrier.",
            ),
            entree("2026-10-21 08-00-00", "2026-10-21", "08:03", "Et vendredi ?", "La démolition des jupettes."),
            entree(
                "2026-10-22 09-14-03",
                "2026-10-22",
                "09:15",
                "La facture de Martin est-elle entrée ?",
                "Oui, le 20 octobre.",
            ),
            // La conversation du 21, reprise le 22 : elle passe en tête.
            entree("2026-10-21 08-00-00", "2026-10-22", "10:02", "Reprenons vendredi", "D'accord."),
        ]
    }

    #[test]
    fn regrouper_par_conversation() {
        let r = regrouper(exemple(), "", 10);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].id, "2026-10-21 08-00-00");
        assert_eq!(r[0].echanges, 3);
        assert_eq!((r[0].date.as_str(), r[0].debut.as_str()), ("2026-10-21", "08:01"));
        assert_eq!((r[0].fin_date.as_str(), r[0].fin.as_str()), ("2026-10-22", "10:02"));
        assert_eq!(r[0].titre, "Qu'est-ce que j'ai demain ?");
        assert_eq!(r[1].id, "2026-10-22 09-14-03");
        assert_eq!(regrouper(exemple(), "", 1).len(), 1);
    }

    #[test]
    fn chercher_sans_egard_aux_accents() {
        let r = regrouper(exemple(), "DEMOLITION", 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].id, "2026-10-21 08-00-00");
        assert_eq!(r[0].extrait.as_deref(), Some("La démolition des jupettes."));
        // Le dernier mot peut n'être qu'un début.
        assert_eq!(regrouper(exemple(), "fact", 10).len(), 1);
        assert_eq!(regrouper(exemple(), "facture martin", 10).len(), 1);
        // Chaque mot doit paraître.
        assert!(regrouper(exemple(), "facture vendredi", 10).is_empty());
        assert!(regrouper(exemple(), "rien de tel", 10).is_empty());
    }

    #[test]
    fn un_extrait_garde_l_ecriture() {
        let texte = "un deux trois quatre cinq six sept huit neuf Écrit dix onze douze treize quatorze quinze seize dix-sept \
                     dix-huit dix-neuf vingt";
        let e = extrait(texte, &["ecrit".to_string()]).unwrap();
        assert!(e.starts_with("… quatre"), "{e}");
        assert!(e.contains("Écrit"));
        assert!(e.ends_with(" …"));
    }

    #[test]
    fn le_bloc_pour_l_adjointe() {
        let b = pour_l_adjointe("Julie", &exemple()[..2]);
        assert!(b.starts_with("[Conversation du 2026-10-21, reprise par Julie"));
        assert!(b.contains("— Julie : « Et vendredi ? » → Eve : « La démolition des jupettes. »"));
        assert!(b.ends_with(']'));
    }

    #[test]
    fn un_nom_de_dossier_sur() {
        assert_eq!(nom_sur("Julie"), "Julie");
        assert_eq!(nom_sur(r"..\..\Windows"), "Windows");
        assert_eq!(nom_sur(""), "poste");
    }
}
