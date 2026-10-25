//! Les rappels et les minuteries d'Eve : « rappelle-moi à 16 h d'appeler le plombier », « minuterie 10 minutes ».
//!
//! Leur seule source est `rappels.json`, dans le dossier local : Eve les y écrit (dits, par la voie rapide de
//! `parole::rappel_dit`, ou posés par Claude dans une carte « rappel » de sa réponse) ; Claude peut le lire et y en
//! retirer un ; Eve le relit quand il change. À l'heure dite, la cloche et la phrase — même après un redémarrage (un
//! rappel manqué pendant qu'Eve était fermée se dit à son retour), même la nuit et en sourdine : on les demande exprès.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eve::parole::Quand;
use serde::{Deserialize, Serialize};

use crate::centre::{date_du_numero, numero_du_jour};

/// Un rappel : son numéro, son heure locale (« 2026-10-25 16:00:00 » : rangée comme un texte, elle se compare comme une
/// heure), ce qu'il rappelle, s'il a été dit ; une minuterie dit son texte tel quel (« La minuterie de 10 minutes est
/// finie. »), un rappel le dit après « Rappel : ».
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rappel {
    pub id: String,
    pub a: String,
    pub texte: String,
    #[serde(default)]
    pub dit: bool,
    #[serde(default)]
    pub minuterie: bool,
}

impl Rappel {
    /// Ce qui se dit à son heure.
    pub fn phrase(&self) -> String {
        if self.minuterie { self.texte.clone() } else { format!("Rappel : {}", self.texte) }
    }
}

/// Les rappels du dossier local, tels que `rappels.json` les tient.
pub struct Rappels {
    chemin: PathBuf,
    liste: Vec<Rappel>,
    /// La date et la taille du fichier lu (ou écrit) : il se relit quand elles changent.
    vu: Option<(SystemTime, u64)>,
}

impl Rappels {
    pub fn ouvrir(local: &Path) -> Rappels {
        let mut r = Rappels { chemin: local.join("rappels.json"), liste: Vec::new(), vu: None };
        r.relire();
        r
    }

    /// Relit le fichier s'il a changé (Claude a pu en retirer un). Illisible, la liste d'avant reste, et le journal le
    /// dit ; disparu, plus de rappels.
    pub fn relire(&mut self) {
        let Ok(m) = std::fs::metadata(&self.chemin) else {
            self.liste.clear();
            self.vu = None;
            return;
        };
        let vu = (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len());
        if self.vu == Some(vu) {
            return;
        }
        self.vu = Some(vu);
        match std::fs::read(&self.chemin).ok().and_then(|o| serde_json::from_slice::<Vec<Rappel>>(&o).ok()) {
            Some(l) => self.liste = l,
            None => crate::journal::ecrire("rappels.json ne se lit pas : les rappels d'avant restent"),
        }
    }

    /// Pose un rappel à `a`. Le même rappel (même heure, même texte) déjà posé ne se double pas.
    pub fn poser(&mut self, a: String, texte: String, minuterie: bool) -> Result<Rappel, String> {
        self.relire();
        if let Some(r) = self.liste.iter().find(|r| r.a == a && r.texte == texte && !r.dit) {
            return Ok(r.clone());
        }
        let suivant =
            self.liste.iter().filter_map(|r| r.id.strip_prefix('r')?.parse::<u64>().ok()).max().unwrap_or(0) + 1;
        let r = Rappel { id: format!("r{suivant}"), a, texte, dit: false, minuterie };
        self.liste.push(r.clone());
        self.liste.sort_by(|x, y| x.a.cmp(&y.a));
        self.ecrire()?;
        Ok(r)
    }

    /// Les rappels dus à `maintenant` (« AAAA-MM-JJ HH:MM:SS ») et pas encore dits : marqués dits sur le disque avant
    /// d'être rendus — un rappel ne se dit jamais deux fois, même si Eve s'arrête en le disant. Les rappels dits depuis
    /// plus d'une semaine s'en vont.
    pub fn sonner(&mut self, maintenant: &str) -> Vec<Rappel> {
        let dus: Vec<Rappel> = self.liste.iter().filter(|r| !r.dit && r.a.as_str() <= maintenant).cloned().collect();
        if dus.is_empty() {
            return dus;
        }
        for r in &mut self.liste {
            r.dit |= r.a.as_str() <= maintenant;
        }
        let semaine = numero_du_jour(maintenant).map(|j| date_du_numero(j - 7)).unwrap_or_default();
        self.liste.retain(|r| !r.dit || r.a.as_str() >= semaine.as_str());
        if let Err(e) = self.ecrire() {
            crate::journal::ecrire(&format!("rappels.json ne s'écrit pas : {e}"));
        }
        dus
    }

    /// Les rappels à venir, du plus proche au plus lointain.
    pub fn a_venir(&self, maintenant: &str) -> Vec<&Rappel> {
        self.liste.iter().filter(|r| !r.dit && r.a.as_str() > maintenant).collect()
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

/// L'heure d'un rappel dit, d'après `maintenant` (« AAAA-MM-JJ HH:MM:SS ») : dans tant de secondes ; ou à une heure de
/// ce jour — de demain si elle est déjà passée, ou si on l'a dit.
pub fn heure_du_rappel(quand: Quand, maintenant: &str) -> Option<String> {
    match quand {
        Quand::Dans(s) => plus_tard(maintenant, i64::from(s)),
        Quand::A { heure, minute, demain } => {
            let jour = numero_du_jour(maintenant)?;
            let a = |j: i64| format!("{} {heure:02}:{minute:02}:00", date_du_numero(j));
            let aujourdhui = a(jour);
            Some(if demain || aujourdhui.as_str() <= maintenant { a(jour + 1) } else { aujourdhui })
        }
    }
}

/// Le moment `secondes` après `moment` (« AAAA-MM-JJ HH:MM:SS »).
fn plus_tard(moment: &str, secondes: i64) -> Option<String> {
    let total = en_secondes(moment)? + secondes;
    let (j, s) = (total.div_euclid(86_400), total.rem_euclid(86_400));
    Some(format!("{} {:02}:{:02}:{:02}", date_du_numero(j), s / 3600, s / 60 % 60, s % 60))
}

/// Un moment (« AAAA-MM-JJ HH:MM:SS ») en secondes depuis l'origine des numéros de jour.
fn en_secondes(moment: &str) -> Option<i64> {
    let jour = numero_du_jour(moment)?;
    let champ = |a: usize, b: usize| moment.get(a..b)?.parse::<i64>().ok();
    Some(jour * 86_400 + champ(11, 13)? * 3600 + champ(14, 16)? * 60 + champ(17, 19)?)
}

/// Un instant Unix (en secondes), à l'heure du poste : « AAAA-MM-JJ HH:MM:SS ». Le décalage du poste se mesure à
/// l'instant même (l'heure locale, moins l'heure Unix) ; l'heure d'été qui change entre-temps n'y est pas.
pub fn heure_locale(unix: i64) -> Option<String> {
    let maintenant = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let decalage = en_secondes(&crate::journal::date_et_heure())? - maintenant;
    let total = unix + decalage;
    let (j, s) = (total.div_euclid(86_400), total.rem_euclid(86_400));
    Some(format!("{} {:02}:{:02}:{:02}", date_du_numero(j), s / 3600, s / 60 % 60, s % 60))
}

/// Les secondes de `de` à `a` (deux moments « AAAA-MM-JJ HH:MM:SS ») ; 0 si l'un ne se lit pas.
pub fn secondes_entre(de: &str, a: &str) -> i64 {
    en_secondes(a).zip(en_secondes(de)).map_or(0, |(a, de)| a - de)
}

/// Une durée telle qu'on la dit : « 30 secondes », « 10 minutes », « 1 heure », « 1 h 30 ».
pub fn duree_en_mots(secondes: u32) -> String {
    let (h, m, s) = (secondes / 3600, secondes / 60 % 60, secondes % 60);
    match (h, m, s) {
        (0, 0, s) => format!("{s} seconde{}", if s > 1 { "s" } else { "" }),
        (0, m, 0) => format!("{m} minute{}", if m > 1 { "s" } else { "" }),
        (0, m, s) => format!("{m} min {s:02}"),
        (h, 0, 0) => format!("{h} heure{}", if h > 1 { "s" } else { "" }),
        (h, m, _) => format!("{h} h {m:02}"),
    }
}

/// L'heure d'un rappel telle qu'on la dit : « 16 h », « 16 h 30 », et « demain à 9 h » si ce n'est pas aujourd'hui
/// (`aujourdhui` : « AAAA-MM-JJ »).
pub fn heure_dite(a: &str, aujourdhui: &str) -> String {
    let (h, m) = (a.get(11..13).unwrap_or("00"), a.get(14..16).unwrap_or("00"));
    let h = h.trim_start_matches('0');
    let h = if h.is_empty() { "0" } else { h };
    let heure = if m == "00" { format!("{h} h") } else { format!("{h} h {m}") };
    let jour = a.get(..10).unwrap_or("");
    if jour == aujourdhui {
        heure
    } else if numero_du_jour(jour).zip(numero_du_jour(aujourdhui)).is_some_and(|(j, a)| j == a + 1) {
        format!("demain à {heure}")
    } else {
        format!("le {} à {heure}", crate::ambiance::date_longue(jour).to_lowercase())
    }
}

/// Une heure de rappel écrite par Claude (« 2026-10-25 16:00 », secondes facultatives), dans la forme du fichier ;
/// `None` si ce n'en est pas une.
pub fn heure_ecrite(a: &str) -> Option<String> {
    let a = a.trim().replace('T', " ");
    let (jour, heure) = a.split_once(' ')?;
    numero_du_jour(jour)?;
    let mut p = heure.split(':');
    let (h, m) = (p.next()?.parse::<u32>().ok()?, p.next()?.parse::<u32>().ok()?);
    let s = p.next().map_or(Some(0), |s| s.get(..2).and_then(|s| s.parse::<u32>().ok()))?;
    (h < 24 && m < 60 && s < 60).then(|| format!("{jour} {h:02}:{m:02}:{s:02}"))
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn l_heure_d_un_rappel() {
        let maintenant = "2026-10-25 15:58:30";
        assert_eq!(heure_du_rappel(Quand::Dans(600), maintenant).unwrap(), "2026-10-25 16:08:30");
        assert_eq!(heure_du_rappel(Quand::Dans(9 * 3600), maintenant).unwrap(), "2026-10-26 00:58:30", "minuit passé");
        let a = |heure, minute, demain| Quand::A { heure, minute, demain };
        assert_eq!(heure_du_rappel(a(16, 0, false), maintenant).unwrap(), "2026-10-25 16:00:00");
        assert_eq!(heure_du_rappel(a(9, 0, false), maintenant).unwrap(), "2026-10-26 09:00:00", "passée : demain");
        assert_eq!(heure_du_rappel(a(16, 0, true), maintenant).unwrap(), "2026-10-26 16:00:00");
        assert_eq!(heure_du_rappel(Quand::Dans(60), "2026-12-31 23:59:30").unwrap(), "2027-01-01 00:00:30");
        assert_eq!(heure_dite("2026-10-25 16:00:00", "2026-10-25"), "16 h");
        assert_eq!(heure_dite("2026-10-26 09:05:00", "2026-10-25"), "demain à 9 h 05");
        assert_eq!(heure_dite("2026-10-30 00:30:00", "2026-10-25"), "le vendredi 30 octobre à 0 h 30");
        assert_eq!(heure_ecrite("2026-10-25 16:00").as_deref(), Some("2026-10-25 16:00:00"));
        assert_eq!(heure_ecrite("2026-10-25T07:05:09").as_deref(), Some("2026-10-25 07:05:09"));
        assert_eq!(heure_ecrite("demain 16 h"), None);
        assert_eq!(heure_ecrite("2026-10-25 25:00"), None);
        assert_eq!(secondes_entre("2026-10-25 23:59:00", "2026-10-26 00:01:30"), 150);
        assert_eq!(duree_en_mots(600), "10 minutes");
        assert_eq!(duree_en_mots(1), "1 seconde");
        assert_eq!(duree_en_mots(5400), "1 h 30");
        assert_eq!(duree_en_mots(7200), "2 heures");
        assert_eq!(duree_en_mots(90), "1 min 30");
    }

    #[test]
    fn les_rappels_sonnent_une_fois_et_survivent_a_un_redemarrage() {
        let dossier = std::env::temp_dir().join(format!("eve-essai-rappels-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        std::fs::create_dir_all(&dossier).unwrap();
        let mut r = Rappels::ouvrir(&dossier);
        let plombier = r.poser("2026-10-25 16:00:00".into(), "appeler le plombier".into(), false).unwrap();
        r.poser("2026-10-25 15:00:00".into(), "sortir le gâteau".into(), false).unwrap();
        let encore = r.poser("2026-10-25 16:00:00".into(), "appeler le plombier".into(), false).unwrap();
        assert_eq!(encore, plombier, "pas deux fois");
        assert_eq!(plombier.phrase(), "Rappel : appeler le plombier");
        assert_eq!(r.a_venir("2026-10-25 14:00:00").len(), 2);
        // Eve redémarre : le fichier les garde ; à 15 h 30, celui de 15 h sonne, une seule fois.
        let mut r = Rappels::ouvrir(&dossier);
        let dus = r.sonner("2026-10-25 15:30:00");
        assert_eq!(dus.iter().map(|x| x.texte.as_str()).collect::<Vec<_>>(), ["sortir le gâteau"]);
        assert!(r.sonner("2026-10-25 15:31:00").is_empty());
        assert!(Rappels::ouvrir(&dossier).sonner("2026-10-25 15:32:00").is_empty(), "dit, même relu");
        // Claude retire celui de 16 h en récrivant le fichier : Eve le relit.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dossier.join("rappels.json"), "[]").unwrap();
        r.relire();
        assert!(r.sonner("2026-10-25 17:00:00").is_empty());
        let _ = std::fs::remove_dir_all(&dossier);
    }
}
