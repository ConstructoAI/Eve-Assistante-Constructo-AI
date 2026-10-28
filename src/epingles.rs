//! Les cartes épinglées au tableau de bord : les notes que la personne y ajoute, et les cartes qu'Eve y épingle — une
//! liste, des chiffres, un tableau, un graphique, un dessin, une note —, comme on épingle une vignette à un tableau de
//! bord (« fais-moi un graphique de mes ventes et mets-le au tableau de bord »).
//!
//! Leur seule source est `epingles.json`, dans le dossier local, et Eve seule l'écrit — le chef, d'un geste : la note
//! qu'on tape dans la fenêtre, la carte « epingle » d'une réponse de Claude ; Claude le lit. Une carte qui revient avec
//! le même `id` remplace l'autre, à sa place : c'est ainsi qu'elle se met à jour ; `"retirer": true` la retire. Un
//! fichier qui ne se lit pas n'est jamais écrasé : rien ne s'y écrit tant qu'il ne se relit pas.
//!
//! Chaque carte est un objet JSON : ce qu'Eve en tient (`id`, `de` — « personne » ou « eve » —, `mis_a_jour`), puis ce
//! que la carte dit d'elle-même (sa `sorte`, son `titre`, sa `teinte`, sa `vue`, son `pied`, sa `source`, les champs de
//! sa sorte), gardé tel quel : la fenêtre le lit en la dessinant, et une sorte qu'elle ne connaît pas encore attend
//! d'elle sa façon d'être dessinée.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Map, Value};

/// Le fichier, dans le dossier local.
pub const FICHIER: &str = "epingles.json";
/// Les bornes d'un tableau de bord : tant de cartes, une carte de tant d'octets (un grand tableau, un dessin), une note
/// de tant de caractères, un `id` de tant de caractères.
const CARTES_MAX: usize = 60;
const CARTE_MAX: usize = 256 * 1024;
const NOTE_MAX: usize = 20_000;
const ID_MAX: usize = 80;
/// Ce qu'Eve tient d'une carte, et que la carte ne dit pas d'elle-même.
const TENUS: [&str; 3] = ["id", "de", "mis_a_jour"];

/// Ce qu'a fait une carte « epingle » de Claude.
#[derive(Debug, PartialEq)]
pub enum Fait {
    /// Posée : son `id`, son titre ; neuve, ou à la place d'une autre.
    Posee { id: String, titre: String, neuve: bool },
    /// Retirée : son `id`, son titre.
    Retiree { id: String, titre: String },
    /// À retirer, mais déjà absente.
    Absente(String),
}

/// Les cartes du dossier local, telles qu'`epingles.json` les tient, dans leur ordre : la plus récente en tête.
pub struct Epingles {
    chemin: PathBuf,
    liste: Vec<Map<String, Value>>,
    /// La date et la taille du fichier lu (ou écrit) : il se relit quand elles changent.
    vu: Option<(SystemTime, u64)>,
    /// Pourquoi le fichier ne se lit pas : tant que c'est vrai, la liste d'avant reste et rien ne s'y écrit.
    illisible: Option<String>,
}

impl Epingles {
    pub fn ouvrir(local: &Path) -> Epingles {
        let mut e = Epingles { chemin: local.join(FICHIER), liste: Vec::new(), vu: None, illisible: None };
        e.relire();
        e
    }

    /// Les cartes, la plus récente en tête.
    pub fn liste(&self) -> &[Map<String, Value>] {
        &self.liste
    }

    /// Relit le fichier s'il a changé (on l'a récrit à la main). Illisible, la liste d'avant reste, le journal le dit
    /// une fois, et rien ne s'y écrit plus ; disparu, plus de cartes. Vrai si la liste a changé.
    pub fn relire(&mut self) -> bool {
        let Ok(m) = std::fs::metadata(&self.chemin) else {
            self.vu = None;
            self.illisible = None;
            return !std::mem::take(&mut self.liste).is_empty();
        };
        let vu = (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len());
        if self.vu == Some(vu) {
            return false;
        }
        self.vu = Some(vu);
        match lire(&self.chemin) {
            Ok(l) => {
                self.illisible = None;
                let change = l != self.liste;
                self.liste = l;
                change
            }
            Err(raison) => {
                if self.illisible.as_ref() != Some(&raison) {
                    crate::journal::ecrire(&format!(
                        "{FICHIER} {raison} : les cartes d'avant restent, et rien ne s'y écrit tant qu'il ne se lit pas"
                    ));
                }
                self.illisible = Some(raison);
                false
            }
        }
    }

    /// La note `id` de la personne, telle qu'elle vient de l'écrire dans la fenêtre : récrite, ou posée en tête si elle
    /// est neuve. Un `id` d'une carte qui n'est pas une note est refusé.
    pub fn noter(&mut self, id: &str, texte: &str, maintenant: &str) -> Result<(), String> {
        self.relire();
        self.ecrivable()?;
        if !id_de_la_fenetre(id) {
            return Err(format!("« {id} » n'est pas le nom d'une note"));
        }
        let texte: String = texte.chars().take(NOTE_MAX).collect();
        match self.liste.iter_mut().find(|c| champ(c, "id") == id) {
            Some(c) if champ(c, "sorte") != "note" => return Err("cette carte n'est pas une note".into()),
            Some(c) => {
                c.insert("texte".into(), Value::String(texte));
                c.insert("mis_a_jour".into(), Value::String(maintenant.into()));
            }
            None => {
                if self.liste.len() >= CARTES_MAX {
                    return Err(format!("le tableau de bord a déjà {CARTES_MAX} cartes"));
                }
                let mut c = Map::new();
                c.insert("id".into(), Value::String(id.into()));
                c.insert("sorte".into(), Value::String("note".into()));
                c.insert("de".into(), Value::String("personne".into()));
                c.insert("texte".into(), Value::String(texte));
                c.insert("mis_a_jour".into(), Value::String(maintenant.into()));
                self.liste.insert(0, c);
            }
        }
        self.ecrire()
    }

    /// Retire la carte `id` (la fenêtre : « Retirer »). Vrai si elle y était.
    pub fn retirer(&mut self, id: &str) -> Result<bool, String> {
        self.relire();
        self.ecrivable()?;
        let avant = self.liste.len();
        self.liste.retain(|c| champ(c, "id") != id);
        if self.liste.len() == avant {
            return Ok(false);
        }
        self.ecrire()?;
        Ok(true)
    }

    /// Une carte « epingle » d'une réponse de Claude (`carte` : son objet, `type` compris) : posée — neuve, en tête ;
    /// avec l'`id` d'une autre, à sa place, qu'elle remplace en gardant de qui elle est (Claude récrit une note de la
    /// personne à sa demande) — ou retirée (`"retirer": true`). Sans `id`, son titre en fait un.
    pub fn epingler(&mut self, mut carte: Map<String, Value>, maintenant: &str) -> Result<Fait, String> {
        self.relire();
        self.ecrivable()?;
        let titre = champ(&carte, "titre").trim().to_string();
        let donne = match carte.get("id") {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        let id = crate::natif::theme::cle_de(if donne.trim().is_empty() { &titre } else { &donne });
        if id.is_empty() {
            return Err("une carte sans id ni titre".into());
        }
        if id.chars().count() > ID_MAX {
            return Err(format!("un id de plus de {ID_MAX} caractères"));
        }
        let place = self.liste.iter().position(|c| champ(c, "id") == id);
        if carte.get("retirer").and_then(Value::as_bool) == Some(true) {
            let Some(k) = place else { return Ok(Fait::Absente(id)) };
            let titre = champ(&self.liste.remove(k), "titre").to_string();
            self.ecrire()?;
            return Ok(Fait::Retiree { id, titre });
        }
        for cle in ["type", "retirer"].into_iter().chain(TENUS) {
            carte.remove(cle);
        }
        if !carte.contains_key("sorte") {
            let sorte = if carte.contains_key("texte") { "note" } else { "liste" };
            carte.insert("sorte".into(), Value::String(sorte.into()));
        }
        let de = place.map_or("eve".to_string(), |k| champ(&self.liste[k], "de").to_string());
        let mut tenue = Map::new();
        tenue.insert("id".into(), Value::String(id.clone()));
        tenue.insert("de".into(), Value::String(if de.is_empty() { "eve".into() } else { de }));
        tenue.insert("mis_a_jour".into(), Value::String(maintenant.into()));
        tenue.extend(carte);
        if serde_json::to_string(&tenue).map_or(0, |s| s.len()) > CARTE_MAX {
            return Err(format!("une carte de plus de {} Ko", CARTE_MAX / 1024));
        }
        let neuve = place.is_none();
        match place {
            Some(k) => self.liste[k] = tenue,
            None if self.liste.len() >= CARTES_MAX => {
                return Err(format!("le tableau de bord a déjà {CARTES_MAX} cartes"));
            }
            None => self.liste.insert(0, tenue),
        }
        self.ecrire()?;
        Ok(Fait::Posee { id, titre, neuve })
    }

    /// Le fichier peut-il s'écrire ? Pas s'il ne se lit pas : sa liste aurait remplacé ce qu'on y a mis.
    fn ecrivable(&self) -> Result<(), String> {
        match &self.illisible {
            Some(raison) => Err(format!("{FICHIER} {raison}")),
            None => Ok(()),
        }
    }

    fn ecrire(&mut self) -> Result<(), String> {
        let texte = serde_json::to_string_pretty(&self.liste).map_err(|e| e.to_string())?;
        crate::reglages::ecrire_d_un_geste(&self.chemin, texte.as_bytes()).map_err(|e| e.to_string())?;
        // Ce qu'Eve vient d'écrire ne se relit pas.
        if let Ok(m) = std::fs::metadata(&self.chemin) {
            self.vu = Some((m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()));
        }
        Ok(())
    }
}

/// Un champ texte d'une carte (vide s'il n'y est pas).
pub fn champ<'a>(c: &'a Map<String, Value>, cle: &str) -> &'a str {
    c.get(cle).and_then(Value::as_str).unwrap_or("")
}

/// Le nom d'une note que donne la fenêtre : des minuscules, des chiffres et des tirets, comme ceux d'Eve.
fn id_de_la_fenetre(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= ID_MAX
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Les cartes du fichier : une liste d'objets JSON (une marque d'ordre des octets en tête passe ; ce qui n'est pas un
/// objet se laisse de côté) ; s'il ne se lit pas, pourquoi, en quelques mots.
fn lire(chemin: &Path) -> Result<Vec<Map<String, Value>>, String> {
    let octets = std::fs::read(chemin).map_err(|_| "ne s'ouvre pas".to_string())?;
    let texte = String::from_utf8(octets).map_err(|_| "n'est pas écrit en UTF-8".to_string())?;
    let v: Value = serde_json::from_str(texte.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("ne se lit pas (ligne {}, colonne {})", e.line(), e.column()))?;
    let Value::Array(l) = v else { return Err("n'est pas une liste JSON".into()) };
    Ok(l.into_iter().filter_map(|c| if let Value::Object(m) = c { Some(m) } else { None }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eve-essai-epingles-{nom}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn carte(json: &str) -> Map<String, Value> {
        serde_json::from_str(json).unwrap()
    }

    /// Une carte de Claude se pose en tête, de lui ; revenue avec le même id, elle remplace l'autre à sa place ; sans
    /// id, son titre en fait un ; `retirer` la retire. Tout se relit du fichier.
    #[test]
    fn une_carte_de_claude_se_pose_se_remplace_et_se_retire() {
        let d = dossier("claude");
        let mut e = Epingles::ouvrir(&d);
        let ventes = r#"{"type":"epingle","id":"ventes","titre":"Ventes","sorte":"chiffres","chiffres":[]}"#;
        assert_eq!(
            e.epingler(carte(ventes), "2026-10-28 09:00:00"),
            Ok(Fait::Posee { id: "ventes".into(), titre: "Ventes".into(), neuve: true })
        );
        let sans_id = r#"{"type":"epingle","titre":"Chantiers à Lévis","sorte":"liste","rangees":[]}"#;
        let fait = e.epingler(carte(sans_id), "2026-10-28 09:01:00");
        assert_eq!(
            fait,
            Ok(Fait::Posee { id: "chantiers-a-levis".into(), titre: "Chantiers à Lévis".into(), neuve: true })
        );
        let ids: Vec<&str> = e.liste().iter().map(|c| champ(c, "id")).collect();
        assert_eq!(ids, ["chantiers-a-levis", "ventes"], "la plus récente en tête");
        let a_jour = r#"{"type":"epingle","id":"ventes","titre":"Ventes de 2026","sorte":"chiffres","de":"personne"}"#;
        let fait = e.epingler(carte(a_jour), "2026-10-28 10:00:00");
        assert_eq!(fait, Ok(Fait::Posee { id: "ventes".into(), titre: "Ventes de 2026".into(), neuve: false }));
        let relue = Epingles::ouvrir(&d);
        let v = &relue.liste()[1];
        assert_eq!(
            (champ(v, "titre"), champ(v, "de"), champ(v, "mis_a_jour")),
            ("Ventes de 2026", "eve", "2026-10-28 10:00:00")
        );
        assert!(!v.contains_key("type") && !v.contains_key("chiffres"), "la carte remplace l'autre tout entière");
        let retirer = r#"{"type":"epingle","id":"ventes","retirer":true}"#;
        assert_eq!(
            e.epingler(carte(retirer), ""),
            Ok(Fait::Retiree { id: "ventes".into(), titre: "Ventes de 2026".into() })
        );
        assert_eq!(e.epingler(carte(retirer), ""), Ok(Fait::Absente("ventes".into())));
        assert!(e.epingler(carte(r#"{"type":"epingle","sorte":"note"}"#), "").is_err(), "ni id ni titre");
        assert_eq!(Epingles::ouvrir(&d).liste().len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// La note de la personne : posée en tête, d'elle ; récrite ; Claude la récrit à sa demande sans qu'elle cesse
    /// d'être à elle ; un id qui n'est pas celui d'une note de la fenêtre, ou celui d'une carte qui n'est pas une note,
    /// est refusé ; retirée.
    #[test]
    fn la_note_de_la_personne_s_ecrit_et_reste_a_elle() {
        let d = dossier("note");
        let mut e = Epingles::ouvrir(&d);
        e.noter("note-1", "Épicerie\n- lait", "2026-10-28 09:00:00").unwrap();
        e.noter("note-1", "Épicerie\n- lait\n- pain", "2026-10-28 09:00:05").unwrap();
        let relue = Epingles::ouvrir(&d);
        let n = &relue.liste()[0];
        assert_eq!(
            (champ(n, "sorte"), champ(n, "de"), champ(n, "texte")),
            ("note", "personne", "Épicerie\n- lait\n- pain")
        );
        let par_claude =
            r#"{"type":"epingle","id":"note-1","sorte":"note","texte":"Épicerie\n- lait\n- pain\n- oeufs"}"#;
        e.epingler(carte(par_claude), "2026-10-28 09:10:00").unwrap();
        assert_eq!(champ(&e.liste()[0], "de"), "personne");
        e.epingler(carte(r#"{"type":"epingle","id":"liste","titre":"Liste","sorte":"liste"}"#), "").unwrap();
        assert!(e.noter("liste", "x", "").is_err(), "pas une note");
        assert!(e.noter("Note 2", "x", "").is_err(), "pas un nom de la fenêtre");
        assert_eq!(e.retirer("note-1"), Ok(true));
        assert_eq!(e.retirer("note-1"), Ok(false));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Un fichier qui ne se lit pas n'est jamais écrasé : les cartes d'avant restent, rien ne s'écrit ; réparé, il se
    /// relit.
    #[test]
    fn un_fichier_illisible_ne_s_ecrase_pas() {
        let d = dossier("illisible");
        let mut e = Epingles::ouvrir(&d);
        e.noter("note-1", "à garder", "").unwrap();
        std::fs::write(d.join(FICHIER), "[{\"id\": \"note-1\",").unwrap();
        e.vu = None;
        assert!(!e.relire());
        assert_eq!(e.liste().len(), 1, "les cartes d'avant restent");
        assert!(e.noter("note-2", "x", "").is_err());
        assert_eq!(std::fs::read_to_string(d.join(FICHIER)).unwrap(), "[{\"id\": \"note-1\",");
        std::fs::write(
            d.join(FICHIER),
            "\u{feff}[{\"id\": \"note-1\", \"sorte\": \"note\", \"texte\": \"réparée\"}, 3]",
        )
        .unwrap();
        e.vu = None;
        assert!(e.relire());
        assert_eq!(champ(&e.liste()[0], "texte"), "réparée");
        assert!(e.noter("note-2", "x", "").is_ok());
        let _ = std::fs::remove_dir_all(&d);
    }
}
