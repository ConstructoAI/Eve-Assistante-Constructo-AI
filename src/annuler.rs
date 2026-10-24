//! « Eve, annule la dernière modification » : avant qu'un fichier change (Write, Edit, MultiEdit, NotebookEdit), Eve en
//! garde une copie dans son dossier local (`annuler\`). Annuler remet les fichiers du dernier tour qui en a changé comme
//! ils étaient ; un fichier que le tour avait créé est retiré — déplacé dans `annuler\`, jamais effacé. La version
//! qu'une annulation remplace y est gardée aussi (« … remplacé … ») : une retouche faite après Eve ne se perd pas, et
//! une annulation de trop se rattrape.
//!
//! Seuls les fichiers écrits par ces outils s'annulent : une commande (Bash, PowerShell), la base ou un courriel ne
//! reviennent pas en arrière par ici. Les copies d'avant ne durent qu'une séance (elles s'effacent au démarrage) ; ce
//! qu'une annulation a retiré ou remplacé reste.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Les outils qui écrivent un fichier, et le champ qui le nomme.
const OUTILS: [(&str, &str); 4] =
    [("Write", "file_path"), ("Edit", "file_path"), ("MultiEdit", "file_path"), ("NotebookEdit", "notebook_path")];

/// Au-delà de ce nombre de copies, les plus anciennes s'oublient.
const COPIES_MAX: usize = 200;

/// Le fichier qu'un appel d'outil va écrire, s'il en écrit un.
pub fn fichier_ecrit(outil: &str, entree: &serde_json::Value) -> Option<PathBuf> {
    let champ = OUTILS.iter().find(|(o, _)| *o == outil)?.1;
    let chemin = entree.get(champ)?.as_str()?.trim();
    (!chemin.is_empty()).then(|| PathBuf::from(chemin))
}

/// Une copie : le fichier, son tour, et sa copie d'avant (`None` : le fichier n'existait pas encore).
struct Copie {
    tour: u64,
    chemin: PathBuf,
    avant: Option<PathBuf>,
}

/// Ce qui a été remis : le fichier ; s'il a été remis comme avant (`true`) ou retiré parce que le tour l'avait créé ;
/// et où sa version remplacée, ou le fichier retiré, est gardé (dans `annuler\`).
pub type Remis = (PathBuf, bool, Option<PathBuf>);

/// Les copies d'une séance.
pub struct Annulations {
    dossier: PathBuf,
    copies: Vec<Copie>,
    numero: u64,
}

impl Annulations {
    /// Les copies vivent dans `dossier`. Celles d'une séance précédente (un numéro seul) s'effacent ici : elles ne
    /// s'annulent plus. Ce qu'une annulation a retiré ou remplacé (« 00007 retiré plan.md ») est le travail de la
    /// personne : il reste, et les numéros repartent après le plus grand, sans écraser rien.
    pub fn nouvelles(dossier: PathBuf) -> Annulations {
        let mut numero = 0;
        if let Ok(lecture) = fs::read_dir(&dossier) {
            for e in lecture.flatten() {
                let nom = e.file_name().to_string_lossy().into_owned();
                if nom.parse::<u64>().is_ok() {
                    let _ = fs::remove_file(e.path());
                } else if let Some(n) = nom.split(' ').next().and_then(|t| t.parse::<u64>().ok()) {
                    numero = numero.max(n);
                }
            }
        }
        Annulations { dossier, copies: Vec::new(), numero }
    }

    /// Le dossier des copies, et de ce qu'une annulation a retiré ou remplacé.
    pub fn dossier(&self) -> &Path {
        &self.dossier
    }

    /// Garde la copie d'avant de `chemin`, une seule fois par tour : c'est l'état d'avant le tour qui compte.
    pub fn garder(&mut self, tour: u64, chemin: &Path) -> io::Result<()> {
        if self.copies.iter().any(|c| c.tour == tour && c.chemin == chemin) {
            return Ok(());
        }
        let avant = if chemin.is_file() {
            self.numero += 1;
            let copie = self.dossier.join(format!("{:05}", self.numero));
            fs::create_dir_all(&self.dossier)?;
            fs::copy(chemin, &copie)?;
            Some(copie)
        } else {
            None
        };
        self.copies.push(Copie { tour, chemin: chemin.to_path_buf(), avant });
        if self.copies.len() > COPIES_MAX {
            let vieille = self.copies.remove(0);
            if let Some(c) = vieille.avant {
                let _ = fs::remove_file(c);
            }
        }
        Ok(())
    }

    /// Y a-t-il quelque chose à annuler ?
    pub fn vide(&self) -> bool {
        self.copies.is_empty()
    }

    /// Remet comme avant les fichiers du dernier tour qui en a changé. Rend ce qui a été remis, et les erreurs.
    ///
    /// Du plus récent au plus ancien : un même fichier nommé de deux façons dans le tour (« C:\X\a.md », « c:\x\a.md »)
    /// a deux copies, et c'est la première — l'état d'avant le tour — qui doit rester.
    ///
    /// Ce qui n'a pas pu être remis (un fichier ouvert dans Excel, un lecteur injoignable) reste à annuler : la demande
    /// suivante le reprend, au lieu de passer au tour d'avant, et sa copie d'avant ne se perd pas.
    pub fn annuler_le_dernier_tour(&mut self) -> (Vec<Remis>, Vec<String>) {
        let Some(tour) = self.copies.last().map(|c| c.tour) else { return (Vec::new(), Vec::new()) };
        let (du_tour, reste): (Vec<Copie>, Vec<Copie>) = self.copies.drain(..).partition(|c| c.tour == tour);
        self.copies = reste;
        let (mut remis, mut erreurs, mut a_reprendre) = (Vec::new(), Vec::new(), Vec::new());
        for c in du_tour.into_iter().rev() {
            match self.remettre(&c) {
                Ok(Some(r)) => remis.push(r),
                Ok(None) => {}
                Err(e) => {
                    erreurs.push(format!("{} : {e}", c.chemin.display()));
                    // Sans sa copie d'avant (effacée à la main), ou devenu un dossier, rien ne se reprendra jamais.
                    let reprenable = match &c.avant {
                        Some(copie) => copie.is_file(),
                        None => !c.chemin.is_dir(),
                    };
                    if reprenable {
                        a_reprendre.push(c);
                    }
                }
            }
        }
        a_reprendre.reverse();
        self.copies.extend(a_reprendre);
        (remis, erreurs)
    }

    /// Remet un fichier du tour. `Ok(None)` : rien à faire (un fichier que le tour avait créé, supprimé depuis).
    fn remettre(&mut self, c: &Copie) -> io::Result<Option<Remis>> {
        match &c.avant {
            Some(copie) => {
                // La version qu'on remplace se garde d'abord : si elle ne peut pas l'être, rien n'est remplacé.
                let gardee = self.mettre_de_cote(&c.chemin, "remplacé", false)?;
                if let Err(e) = fs::copy(copie, &c.chemin) {
                    // Rien n'a été remplacé : la copie mise de côté serait trompeuse.
                    if let Some(g) = &gardee {
                        effacer(g);
                    }
                    return Err(e);
                }
                Ok(Some((c.chemin.clone(), true, gardee)))
            }
            None if c.chemin.is_dir() => Err(io::Error::other("c'est maintenant un dossier : il n'est pas retiré")),
            None if c.chemin.exists() => {
                Ok(self.mettre_de_cote(&c.chemin, "retiré", true)?.map(|g| (c.chemin.clone(), false, Some(g))))
            }
            // Absent : supprimé depuis, rien à faire — sauf si son dossier même est injoignable (un lecteur réseau
            // débranché) ; le fichier reviendrait avec lui.
            None if c.chemin.parent().is_some_and(|p| !p.as_os_str().is_empty() && !p.exists()) => {
                Err(io::Error::new(io::ErrorKind::NotFound, "son dossier est injoignable"))
            }
            None => Ok(None),
        }
    }

    /// Met `chemin` de côté dans le dossier, sous un numéro neuf (« 00008 remplacé plan.md ») : déplacé pour un fichier
    /// retiré, copié pour une version qu'on va remplacer. Rend où il est gardé ; un fichier absent n'a rien à garder.
    fn mettre_de_cote(&mut self, chemin: &Path, sorte: &str, deplacer_le: bool) -> io::Result<Option<PathBuf>> {
        if !chemin.is_file() {
            return Ok(None);
        }
        self.numero += 1;
        let nom = chemin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let cible = self.dossier.join(format!("{:05} {sorte} {}", self.numero, raccourcir(&nom)));
        fs::create_dir_all(&self.dossier)?;
        if deplacer_le {
            fs::rename(chemin, &cible).or_else(|_| deplacer(chemin, &cible))?;
        } else {
            fs::copy(chemin, &cible)?;
        }
        Ok(Some(cible))
    }
}

/// Un nom gardé tient, avec son préfixe (« 00008 remplacé »), sous la limite de Windows (255 caractères) : au-delà de
/// 200, le début du nom reste, avec son extension.
fn raccourcir(nom: &str) -> String {
    const MAX: usize = 200;
    let longueur = |s: &str| s.encode_utf16().count();
    if longueur(nom) <= MAX {
        return nom.to_string();
    }
    let (base, ext) = match nom.rfind('.') {
        Some(i) if i > 0 && nom.len() - i <= 16 => nom.split_at(i),
        _ => (nom, ""),
    };
    let mut garde = String::new();
    for c in base.chars() {
        if longueur(&garde) + c.len_utf16() + 1 + longueur(ext) > MAX {
            break;
        }
        garde.push(c);
    }
    format!("{garde}…{ext}")
}

/// Un déplacement d'un disque à l'autre (rename ne le fait pas) : copier, puis retirer l'original. Si l'original ne
/// se retire pas (ouvert ailleurs), la copie s'efface : rien ne traîne dans `annuler\` pour un fichier resté en place.
fn deplacer(de: &Path, vers: &Path) -> io::Result<()> {
    fs::copy(de, vers)?;
    fs::remove_file(de).inspect_err(|_| effacer(vers))
}

/// Efface une copie faite ici, même en lecture seule (la copie d'un fichier en lecture seule l'est aussi, et Windows
/// refuse alors de l'effacer).
#[allow(clippy::permissions_set_readonly_false)]
fn effacer(p: &Path) {
    if let Ok(m) = fs::metadata(p) {
        let mut droits = m.permissions();
        if droits.readonly() {
            droits.set_readonly(false);
            let _ = fs::set_permissions(p, droits);
        }
    }
    let _ = fs::remove_file(p);
}

#[cfg(test)]
mod essais {
    use super::*;
    use serde_json::json;

    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eve-annuler-{nom}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Le fichier et s'il est remis comme avant : sans l'endroit où sa version est gardée.
    fn fichiers(remis: &[Remis]) -> Vec<(PathBuf, bool)> {
        remis.iter().map(|(f, comme_avant, _)| (f.clone(), *comme_avant)).collect()
    }

    #[test]
    fn le_fichier_ecrit_se_reconnait() {
        assert_eq!(fichier_ecrit("Write", &json!({"file_path": "C:/x/a.md"})), Some(PathBuf::from("C:/x/a.md")));
        assert_eq!(fichier_ecrit("NotebookEdit", &json!({"notebook_path": "b.ipynb"})), Some(PathBuf::from("b.ipynb")));
        assert_eq!(fichier_ecrit("Bash", &json!({"command": "rm a"})), None);
        assert_eq!(fichier_ecrit("Edit", &json!({"file_path": "  "})), None);
    }

    #[test]
    fn annuler_remet_le_dernier_tour_seulement() {
        let d = dossier("tours");
        let a = d.join("a.txt");
        let b = d.join("b.txt");
        fs::write(&a, "a1").unwrap();
        let mut an = Annulations::nouvelles(d.join("copies"));
        assert!(an.vide());

        an.garder(1, &a).unwrap();
        fs::write(&a, "a2").unwrap();
        an.garder(2, &a).unwrap();
        an.garder(2, &b).unwrap();
        fs::write(&a, "a3").unwrap();
        an.garder(2, &a).unwrap(); // une seconde écriture du même tour : la copie d'avant le tour reste
        fs::write(&a, "a4").unwrap();
        fs::write(&b, "nouveau").unwrap();

        let (remis, erreurs) = an.annuler_le_dernier_tour();
        assert!(erreurs.is_empty(), "{erreurs:?}");
        assert_eq!(remis.len(), 2);
        assert_eq!(fs::read_to_string(&a).unwrap(), "a2");
        assert!(!b.exists(), "le fichier créé par le tour est retiré");
        assert!(
            fs::read_dir(d.join("copies")).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("b.txt"))
        );

        let (remis, _) = an.annuler_le_dernier_tour();
        assert_eq!(fichiers(&remis), vec![(a.clone(), true)]);
        assert_eq!(fs::read_to_string(&a).unwrap(), "a1");
        assert!(an.vide());
        let _ = fs::remove_dir_all(&d);
    }

    /// Une retouche faite après Eve n'est pas perdue : la version que l'annulation remplace est gardée.
    #[test]
    fn une_annulation_garde_la_version_qu_elle_remplace() {
        let d = dossier("remplace");
        let a = d.join("devis.md");
        fs::write(&a, "avant Eve").unwrap();
        let mut an = Annulations::nouvelles(d.join("copies"));
        an.garder(1, &a).unwrap();
        fs::write(&a, "écrit par Eve").unwrap();
        fs::write(&a, "écrit par Eve, retouché par la personne").unwrap();

        let (remis, erreurs) = an.annuler_le_dernier_tour();
        assert!(erreurs.is_empty(), "{erreurs:?}");
        assert_eq!(fichiers(&remis), vec![(a.clone(), true)]);
        assert_eq!(fs::read_to_string(&a).unwrap(), "avant Eve");
        let gardees: Vec<PathBuf> = fs::read_dir(an.dossier())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().contains("remplacé devis.md"))
            .collect();
        assert_eq!(gardees.len(), 1, "{gardees:?}");
        assert_eq!(remis[0].2.as_ref(), Some(&gardees[0]), "la remise dit où sa version est gardée");
        assert_eq!(fs::read_to_string(&gardees[0]).unwrap(), "écrit par Eve, retouché par la personne");
        let _ = fs::remove_dir_all(&d);
    }

    /// La version courante ne peut pas être mise de côté (sa place est prise) : rien n'est remplacé, et la remise
    /// reste à faire — la demande suivante la reprend, au lieu de passer au tour d'avant.
    #[test]
    fn une_mise_de_cote_ratee_ne_remplace_rien_et_se_reprend() {
        let d = dossier("cote-ratee");
        let (a, b) = (d.join("a.txt"), d.join("b.txt"));
        fs::write(&a, "a avant").unwrap();
        fs::write(&b, "b avant").unwrap();
        let mut an = Annulations::nouvelles(d.join("copies"));
        an.garder(1, &b).unwrap(); // un tour plus ancien, qui ne doit pas être touché
        fs::write(&b, "b écrit par Eve").unwrap();
        an.garder(2, &a).unwrap(); // copie d'avant : « 00002 »
        fs::write(&a, "a écrit par Eve").unwrap();
        // La place de la version mise de côté (« 00003 remplacé a.txt ») est prise par un dossier.
        let bouchon = an.dossier().join("00003 remplacé a.txt");
        fs::create_dir_all(&bouchon).unwrap();

        let (remis, erreurs) = an.annuler_le_dernier_tour();
        assert!(remis.is_empty() && erreurs.len() == 1, "{remis:?} {erreurs:?}");
        assert_eq!(fs::read_to_string(&a).unwrap(), "a écrit par Eve", "rien n'est remplacé sans la mise de côté");
        assert_eq!(fs::read_to_string(&b).unwrap(), "b écrit par Eve", "le tour d'avant n'est pas touché");

        fs::remove_dir(&bouchon).unwrap();
        let (remis, erreurs) = an.annuler_le_dernier_tour();
        assert!(erreurs.is_empty(), "{erreurs:?}");
        assert_eq!(fichiers(&remis), vec![(a.clone(), true)], "la même remise est reprise");
        assert_eq!(fs::read_to_string(&a).unwrap(), "a avant");
        assert_eq!(fs::read_to_string(&b).unwrap(), "b écrit par Eve");
        let _ = fs::remove_dir_all(&d);
    }

    /// La remise même échoue (le fichier en lecture seule, comme un fichier ouvert ailleurs) : le fichier reste en place,
    /// tel qu'Eve l'a écrit, aucune copie « remplacé » trompeuse ne reste, et la remise se reprend.
    #[test]
    #[allow(clippy::permissions_set_readonly_false)]
    fn une_remise_ratee_laisse_le_fichier_en_place_et_se_reprend() {
        let d = dossier("remise-ratee");
        let a = d.join("contrat.txt");
        fs::write(&a, "avant").unwrap();
        let mut an = Annulations::nouvelles(d.join("copies"));
        an.garder(1, &a).unwrap();
        fs::write(&a, "écrit par Eve").unwrap();
        let mut droits = fs::metadata(&a).unwrap().permissions();
        droits.set_readonly(true);
        fs::set_permissions(&a, droits.clone()).unwrap();

        let (remis, erreurs) = an.annuler_le_dernier_tour();
        assert!(remis.is_empty() && erreurs.len() == 1, "{remis:?} {erreurs:?}");
        assert_eq!(fs::read_to_string(&a).unwrap(), "écrit par Eve", "le fichier reste à sa place");
        let traine =
            fs::read_dir(an.dossier()).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains("remplacé"));
        assert!(!traine, "pas de copie « remplacé » pour un fichier qui n'a pas été remplacé");
        assert!(!an.vide(), "la remise reste à faire");

        droits.set_readonly(false);
        fs::set_permissions(&a, droits).unwrap();
        let (remis, erreurs) = an.annuler_le_dernier_tour();
        assert!(erreurs.is_empty(), "{erreurs:?}");
        assert_eq!(fichiers(&remis), vec![(a.clone(), true)]);
        assert_eq!(fs::read_to_string(&a).unwrap(), "avant");
        let _ = fs::remove_dir_all(&d);
    }

    /// Un fichier modifié par Eve puis supprimé par la personne revient comme il était avant le tour.
    #[test]
    fn un_fichier_supprime_apres_eve_revient() {
        let d = dossier("supprime");
        let a = d.join("notes.md");
        fs::write(&a, "avant").unwrap();
        let mut an = Annulations::nouvelles(d.join("copies"));
        an.garder(1, &a).unwrap();
        fs::write(&a, "écrit par Eve").unwrap();
        fs::remove_file(&a).unwrap();
        let (remis, erreurs) = an.annuler_le_dernier_tour();
        assert!(erreurs.is_empty(), "{erreurs:?}");
        assert_eq!(remis, vec![(a.clone(), true, None)]);
        assert_eq!(fs::read_to_string(&a).unwrap(), "avant");
        let _ = fs::remove_dir_all(&d);
    }

    /// Un fichier créé par le tour dans un dossier devenu injoignable (un lecteur débranché) : l'erreur le dit, et le
    /// retrait se reprend quand le dossier revient.
    #[test]
    fn un_dossier_injoignable_se_reprend() {
        let d = dossier("injoignable");
        let (x, ailleurs) = (d.join("Lecteur"), d.join("Lecteur-debranche"));
        fs::create_dir_all(&x).unwrap();
        let neuf = x.join("devis neuf.md");
        let mut an = Annulations::nouvelles(d.join("copies"));
        an.garder(1, &neuf).unwrap();
        fs::write(&neuf, "créé par Eve").unwrap();
        fs::rename(&x, &ailleurs).unwrap();
        let (remis, erreurs) = an.annuler_le_dernier_tour();
        assert!(remis.is_empty() && erreurs.len() == 1, "{remis:?} {erreurs:?}");
        fs::rename(&ailleurs, &x).unwrap();
        let (remis, erreurs) = an.annuler_le_dernier_tour();
        assert!(erreurs.is_empty(), "{erreurs:?}");
        assert_eq!(fichiers(&remis), vec![(neuf.clone(), false)]);
        assert!(!neuf.exists());
        let _ = fs::remove_dir_all(&d);
    }

    /// Un nom long (250 caractères) s'annule : son nom gardé est raccourci sous la limite de Windows, extension comprise.
    #[test]
    fn un_nom_long_s_annule() {
        let d = dossier("long");
        let nom = format!("{}.docx", "x".repeat(245));
        let a = d.join(&nom);
        fs::write(&a, "avant").unwrap();
        let mut an = Annulations::nouvelles(d.join("copies"));
        an.garder(1, &a).unwrap();
        fs::write(&a, "écrit par Eve").unwrap();
        let (remis, erreurs) = an.annuler_le_dernier_tour();
        assert!(erreurs.is_empty(), "{erreurs:?}");
        assert_eq!(fs::read_to_string(&a).unwrap(), "avant");
        let gardee = remis[0].2.clone().unwrap();
        let nom_garde = gardee.file_name().unwrap().to_string_lossy().into_owned();
        assert!(nom_garde.encode_utf16().count() <= 255 && nom_garde.ends_with(".docx"), "{nom_garde}");
        assert_eq!(fs::read_to_string(&gardee).unwrap(), "écrit par Eve");
        assert_eq!(raccourcir("court.pdf"), "court.pdf");
        let _ = fs::remove_dir_all(&d);
    }

    /// Au redémarrage, les copies d'avant s'effacent ; ce qui a été retiré ou remplacé reste tel quel, et la séance
    /// suivante, refaisant les mêmes gestes, numérote après : rien ne s'écrase.
    #[test]
    fn ce_qui_est_retire_ou_remplace_survit_au_redemarrage() {
        let d = dossier("redemarrage");
        let copies = d.join("copies");
        let (a, b) = (d.join("a.txt"), d.join("b.txt"));
        // Une séance : b créé puis retiré ; a modifié puis remis comme avant.
        let seance = |an: &mut Annulations, quoi: &str| {
            an.garder(1, &b).unwrap();
            fs::write(&b, format!("b {quoi}")).unwrap();
            assert!(an.annuler_le_dernier_tour().1.is_empty());
            an.garder(2, &a).unwrap();
            fs::write(&a, format!("a {quoi}")).unwrap();
            assert!(an.annuler_le_dernier_tour().1.is_empty());
        };
        let contenus = || -> Vec<(String, String)> {
            let mut v: Vec<(String, String)> = fs::read_dir(&copies)
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    (e.file_name().to_string_lossy().into_owned(), fs::read_to_string(e.path()).unwrap_or_default())
                })
                .collect();
            v.sort();
            v
        };
        fs::write(&a, "a d'origine").unwrap();
        let mut an = Annulations::nouvelles(copies.clone());
        seance(&mut an, "première");
        let gardes: Vec<(String, String)> = contenus().into_iter().filter(|(n, _)| n.parse::<u64>().is_err()).collect();
        assert_eq!(gardes.len(), 2, "{gardes:?}");

        let mut an = Annulations::nouvelles(copies.clone());
        assert_eq!(contenus(), gardes, "seules les copies d'avant s'effacent au redémarrage");
        seance(&mut an, "seconde");
        let fin = contenus();
        assert!(gardes.iter().all(|g| fin.contains(g)), "{fin:?}");
        assert_eq!(fin.iter().filter(|(n, _)| n.parse::<u64>().is_err()).count(), 4, "{fin:?}");
        assert_eq!(fs::read_to_string(&a).unwrap(), "a d'origine");
        let _ = fs::remove_dir_all(&d);
    }

    /// Un même fichier nommé de deux façons dans un tour (NTFS ne distingue pas la casse) : c'est l'état d'avant le
    /// tour qui revient, pas celui d'entre les deux écritures.
    #[cfg(windows)]
    #[test]
    fn deux_noms_du_meme_fichier_remettent_l_etat_d_avant_le_tour() {
        let d = dossier("deux-noms");
        let a = d.join("plan.md");
        let a_autrement = d.join("PLAN.MD");
        fs::write(&a, "avant").unwrap();
        let mut an = Annulations::nouvelles(d.join("copies"));
        an.garder(1, &a).unwrap();
        fs::write(&a, "première écriture").unwrap();
        an.garder(1, &a_autrement).unwrap();
        fs::write(&a_autrement, "seconde écriture").unwrap();
        // Le fondement de l'essai : sur ce volume, les deux noms désignent le même fichier (sinon il passerait à tort).
        assert_eq!(
            fs::read_to_string(&a).unwrap(),
            "seconde écriture",
            "volume sensible à la casse : essai sans objet"
        );
        assert!(an.annuler_le_dernier_tour().1.is_empty());
        assert_eq!(fs::read_to_string(&a).unwrap(), "avant");
        let _ = fs::remove_dir_all(&d);
    }
}
