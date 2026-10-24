//! Les profils d'une Eve (demandés le 24 octobre 2026) : en plus de son dossier de travail, d'autres
//! dossiers, qu'elle crée elle-même et qu'on choisit. Un profil porte son `CLAUDE.md` (d'ordinaire un `PROFIL.md` qu'il
//! importe, peut-être sa consigne vocale, `EVE_CONSIGNE.md`) : la forme du dossier d'un spécialiste du kit. Celui
//! d'office est le dossier de travail de l'Eve elle-même, nommé par son nom ([`nom_affiche`]) : toujours là, jamais
//! inscrit.
//!
//! La liste vit à un seul endroit, `reglages.json` : `profils` (un nom et un dossier chacun) et `profil`, le nom du
//! profil choisi (vide : celui d'office). Les commandes ([`commande`]) y écrivent comme un réglage changé dans la
//! fenêtre ([`Reglages::changer`]) ; l'Eve en marche relit le fichier et passe au profil choisi entre deux demandes :
//! Claude Code repart dans son dossier, et chaque profil garde sa conversation ([`Laisses`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::reglages::{self, Profil, Reglages, nom_affiche};

/// Un nom de profil a quarante caractères au plus, comme le nom d'une Eve.
const NOM_MAX: usize = 40;

/// Comment s'écrivent les commandes, quand elles sont mal écrites.
const USAGE: &str = "eve --profils | --profil-ajouter \"<nom>\" \"<dossier>\" | --profil [\"<nom>\"] | \
                     --profil-retirer \"<nom>\"";

/// Ce qui manque au dossier d'un profil pour qu'Eve y travaille ; `None` : il est prêt (il existe et porte son
/// `CLAUDE.md`, que Claude Code y prend).
pub fn manque(dossier: &Path) -> Option<String> {
    if !dossier.is_dir() {
        Some(format!("le dossier {} n'existe pas", dossier.display()))
    } else if !dossier.join("CLAUDE.md").is_file() {
        Some(format!("le dossier {} n'a pas de CLAUDE.md", dossier.display()))
    } else {
        None
    }
}

/// Deux noms de profil sont-ils le même ? Sans égard à la casse ni aux blancs autour : « estimateur » dit à voix haute
/// désigne « Estimateur ».
pub fn meme(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Deux dossiers sont-ils le même ? Sans égard à la casse, ni aux séparateurs (Windows ne les distingue pas).
pub fn meme_dossier(a: &Path, b: &Path) -> bool {
    let forme = |d: &Path| d.components().collect::<PathBuf>().to_string_lossy().to_lowercase();
    forme(a) == forme(b)
}

/// Deux profils actifs sont-ils le même : le même nom, dans le même dossier ? `None` : celui d'office.
pub fn meme_profil(a: Option<&Profil>, b: Option<&Profil>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => meme(&a.nom, &b.nom) && meme_dossier(&a.dossier, &b.dossier),
        _ => false,
    }
}

/// Les profils de la liste, sans celui qui porterait le nom du profil d'office (écrit à la main) : ce nom-là désigne
/// toujours le dossier d'Eve elle-même.
pub fn liste<'a>(r: &'a Reglages, office: &'a str) -> impl Iterator<Item = &'a Profil> {
    r.profils.iter().filter(move |p| !meme(&p.nom, office))
}

/// Le profil que nomme `nom` : `Ok(None)` pour celui d'office (son nom, ou rien) ; `Ok(Some(…))` pour un profil de la
/// liste dont le dossier est prêt ; sinon, pourquoi il ne peut pas servir.
pub fn trouver(r: &Reglages, office: &str, nom: &str) -> Result<Option<Profil>, String> {
    if nom.trim().is_empty() || meme(nom, office) {
        return Ok(None);
    }
    let Some(p) = liste(r, office).find(|p| meme(&p.nom, nom)) else {
        return Err(format!("Aucun profil « {} » ; ceux de cette Eve : {}.", nom.trim(), noms(r, office)));
    };
    match manque(&p.dossier) {
        Some(m) => Err(format!("Le profil {} ne peut pas servir : {m}.", p.nom)),
        None => Ok(Some(p.clone())),
    }
}

/// Les noms des profils, celui d'office d'abord : « Assistante (d'office), Estimateur ».
fn noms(r: &Reglages, office: &str) -> String {
    std::iter::once(format!("{office} (d'office)"))
        .chain(liste(r, office).map(|p| p.nom.clone()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Un nom de profil tel qu'il s'inscrit : ébarbé, d'un à quarante caractères, sans caractère de contrôle.
fn nom_propre(nom: &str) -> Result<String, String> {
    let n = nom.trim();
    if n.is_empty() {
        Err("Un profil a besoin d'un nom.".into())
    } else if n.chars().count() > NOM_MAX || n.chars().any(char::is_control) {
        Err(format!("« {n} » ne va pas comme nom de profil : quarante caractères au plus, sur une ligne."))
    } else {
        Ok(n.to_string())
    }
}

// ---- Les commandes ----------------------------------------------------------------------------------------------

/// Une commande des profils, si `args` en est une : elle travaille sur `reglages.json` du dossier local
/// (`EVE_DOSSIER_LOCAL`, que Claude Code hérite de l'Eve qui le lance : l'Eve en marche relit ce qu'elle y écrit), dit
/// en une ligne ce qu'elle a fait — ou pourquoi elle refuse, sur la sortie des erreurs — et rend son code de sortie
/// (0 : fait). Ni le journal ni la vitrine ne s'ouvrent : elles sortent avant.
pub fn commande(args: &[String]) -> Option<i32> {
    let (premier, reste) = args.split_first()?;
    let office = nom_affiche();
    // Un nom pris en plusieurs morceaux (sans guillemets autour) se recolle.
    let nom = reste.join(" ");
    let fait = match (premier.as_str(), reste) {
        ("--profils", []) => reglages::lire_du_poste().map(|r| lister(&r, office, r.dossier_adjointe().as_deref())),
        ("--profil-ajouter", [nom, dossier]) => {
            reglages::a_changer().and_then(|mut r| ajouter(&mut r, office, nom, Path::new(dossier)))
        }
        ("--profil", _) => reglages::a_changer().and_then(|mut r| choisir(&mut r, office, &nom)),
        ("--profil-retirer", [_, ..]) => reglages::a_changer().and_then(|mut r| retirer(&mut r, office, &nom)),
        ("--profils" | "--profil-ajouter" | "--profil-retirer", _) => Err(USAGE.to_string()),
        _ => return None,
    };
    Some(reglages::sortie_de_commande(fait))
}

/// `eve --profils` : celui d'office d'abord, puis la liste, le profil choisi marqué d'une étoile ; un profil dont le
/// dossier ne peut pas servir le dit.
pub fn lister(r: &Reglages, office: &str, dossier_d_office: Option<&Path>) -> String {
    let choisi = liste(r, office).find(|p| meme(&p.nom, &r.profil));
    let marque = |oui: bool| if oui { "*" } else { " " };
    let ou = dossier_d_office.map_or_else(|| "le dossier d'Eve".to_string(), |d| d.display().to_string());
    let mut s = format!(
        "Les profils de cette Eve (dans {}) ; * : le profil choisi.\n{} {office} (d'office) : {ou}",
        r.chemin().display(),
        marque(choisi.is_none())
    );
    for p in liste(r, office) {
        let etat = manque(&p.dossier).map(|m| format!(" — {m}")).unwrap_or_default();
        let actif = choisi.is_some_and(|c| std::ptr::eq(c, p));
        s.push_str(&format!("\n{} {} : {}{etat}", marque(actif), p.nom, p.dossier.display()));
    }
    s
}

/// `eve --profil-ajouter <nom> <dossier>` : le dossier doit exister et porter son `CLAUDE.md` ; jamais le nom du
/// profil d'office. Un nom déjà pris (sans égard à la casse) reste unique : son profil prend ce dossier.
pub fn ajouter(r: &mut Reglages, office: &str, nom: &str, dossier: &Path) -> Result<String, String> {
    let nom = nom_propre(nom)?;
    if meme(&nom, office) {
        return Err(format!(
            "« {nom} » est le nom du profil d'office, le dossier d'Eve elle-même : choisis-en un autre."
        ));
    }
    // Un chemin relatif part du dossier courant (celui où travaille Claude Code) ; il s'inscrit complet.
    let dossier = std::path::absolute(dossier)
        .map_err(|e| format!("Le profil {nom} n'est pas ajouté : le dossier {} ({e}).", dossier.display()))?
        .components()
        .collect::<PathBuf>();
    if let Some(m) = manque(&dossier) {
        return Err(format!("Le profil {nom} n'est pas ajouté : {m}."));
    }
    let mut profils = r.profils.clone();
    let fait = match profils.iter_mut().find(|p| meme(&p.nom, &nom)) {
        Some(p) if p.nom == nom && meme_dossier(&p.dossier, &dossier) => {
            return Ok(format!("Le profil {nom} est déjà là : {}.", dossier.display()));
        }
        Some(p) => {
            let avant = std::mem::replace(&mut p.dossier, dossier.clone());
            p.nom.clone_from(&nom);
            format!("Le profil {nom} prend le dossier {} (avant : {}).", dossier.display(), avant.display())
        }
        None => {
            profils.push(Profil { nom: nom.clone(), dossier: dossier.clone() });
            format!("Le profil {nom} est ajouté : {} ; « --profil {nom} » le rend actif.", dossier.display())
        }
    };
    let valeur = serde_json::to_value(&profils).map_err(|e| e.to_string())?;
    r.changer("profils", valeur)?;
    Ok(fait)
}

/// `eve --profil [<nom>]` : le profil choisi, qu'Eve prend entre deux demandes ; le nom de celui d'office, ou rien, y
/// ramène. Un profil dont le dossier ne peut pas servir n'est pas choisi.
pub fn choisir(r: &mut Reglages, office: &str, nom: &str) -> Result<String, String> {
    let choix = trouver(r, office, nom)?;
    let deja = match &choix {
        Some(p) => meme(&r.profil, &p.nom),
        None => r.profil.trim().is_empty() || meme(&r.profil, office),
    };
    let (valeur, dit) = match &choix {
        Some(p) => (p.nom.clone(), format!("Le profil {} ({})", p.nom, p.dossier.display())),
        None => (String::new(), format!("Le profil d'office, {office},")),
    };
    r.changer("profil", json!(valeur))?;
    Ok(if deja {
        format!("{dit} est déjà le profil choisi.")
    } else {
        format!("{dit} est choisi : Eve y passe entre deux demandes.")
    })
}

/// `eve --profil-retirer <nom>` : le profil quitte la liste, son dossier reste tel quel ; s'il était choisi, Eve
/// revient à son profil d'office.
pub fn retirer(r: &mut Reglages, office: &str, nom: &str) -> Result<String, String> {
    // Un profil inscrit d'abord : même s'il porte le nom de celui d'office (écrit à la main), il se retire.
    let Some(i) = r.profils.iter().position(|p| meme(&p.nom, nom)) else {
        return Err(if nom.trim().is_empty() || meme(nom, office) {
            format!("Le profil d'office, {office}, ne se retire pas : c'est le dossier d'Eve elle-même.")
        } else {
            format!("Aucun profil « {} » à retirer ; ceux de cette Eve : {}.", nom.trim(), noms(r, office))
        });
    };
    let mut profils = r.profils.clone();
    let p = profils.remove(i);
    let choisi = meme(&r.profil, &p.nom);
    // Le profil choisi d'abord : coupé entre les deux écritures, le fichier ne choisirait pas un profil retiré.
    if choisi {
        r.changer("profil", json!(""))?;
    }
    let valeur = serde_json::to_value(&profils).map_err(|e| e.to_string())?;
    r.changer("profils", valeur)?;
    let suite = if choisi {
        " C'était le profil choisi : Eve revient à son profil d'office entre deux demandes."
    } else {
        ""
    };
    Ok(format!(
        "Le profil {} est retiré de la liste ; son dossier, {}, reste tel quel.{suite}",
        p.nom,
        p.dossier.display()
    ))
}

// ---- Une conversation par profil --------------------------------------------------------------------------------

/// Ce que laisse un profil quand on le quitte : sa séance de Claude Code, que `--resume` reprend à son retour, et sa
/// conversation de l'historique ; et le dossier où elles vivent (Claude Code range ses séances par dossier).
#[derive(Debug, Clone, PartialEq)]
pub struct Laisse {
    pub dossier: PathBuf,
    pub session: Option<String>,
    pub conversation: String,
}

/// Ce que les profils quittés ont laissé, chacun par son nom (celui d'office : par rien) : le temps qu'Eve tourne.
#[derive(Debug, Default)]
pub struct Laisses(HashMap<String, Laisse>);

impl Laisses {
    /// Eve quitte le profil `de` (son nom ; `None` : celui d'office), qui laisse `laisse`, pour le profil `vers`, qui
    /// travaille dans `dossier` : rend ce que `vers` avait laissé — rien s'il a changé de dossier depuis, sa séance
    /// d'avant vivant dans l'autre.
    pub fn echanger(&mut self, de: Option<&str>, laisse: Laisse, vers: Option<&str>, dossier: &Path) -> Option<Laisse> {
        self.0.insert(cle(de), laisse);
        self.0.remove(&cle(vers)).filter(|l| meme_dossier(&l.dossier, dossier))
    }

    /// Une autre personne devant le poste : les conversations des profils étaient celles de la précédente.
    pub fn oublier(&mut self) {
        self.0.clear();
    }
}

fn cle(nom: Option<&str>) -> String {
    nom.map(|n| n.trim().to_lowercase()).unwrap_or_default()
}

#[cfg(test)]
mod essais {
    use super::*;
    use std::fs;

    /// Un dossier d'essai neuf, sous le dossier temporaire : jamais celui d'une vraie Eve.
    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eve-profils-{nom}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// Un dossier de profil prêt : il porte son CLAUDE.md.
    fn pret(sous: &Path, nom: &str) -> PathBuf {
        let d = sous.join(nom);
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("CLAUDE.md"), "@PROFIL.md\n").unwrap();
        d
    }

    /// Les réglages d'un reglages.json d'essai qui dit `texte`.
    fn reglages(d: &Path, texte: &str) -> Reglages {
        let chemin = d.join("reglages.json");
        fs::write(&chemin, texte).unwrap();
        reglages::charger_de(chemin)
    }

    fn relus(r: &Reglages) -> Reglages {
        reglages::charger_de(r.chemin().to_path_buf())
    }

    #[test]
    fn les_profils_se_lisent_dans_reglages_json() {
        let r: Reglages = serde_json::from_str(
            r#"{"nom": "Essai", "profils": [{"nom": "Estimateur", "dossier": "C:\\Exemple\\Estimation"}, {"nom": "Sans"}],
                "profil": "estimateur"}"#,
        )
        .unwrap();
        assert_eq!(r.profils[0], Profil { nom: "Estimateur".into(), dossier: PathBuf::from(r"C:\Exemple\Estimation") });
        assert_eq!(r.profils[1].dossier, PathBuf::new(), "un profil sans dossier se lit : il ne peut pas servir");
        assert_eq!(r.profil, "estimateur");
        let ancien: Reglages = serde_json::from_str(r#"{"nom": "Assistante"}"#).unwrap();
        assert!(ancien.profils.is_empty() && ancien.profil.is_empty(), "un reglages.json d'avant : celui d'office");
    }

    #[test]
    fn trouver_un_profil() {
        let d = dossier("trouver");
        let estimation = pret(&d, "Estimation");
        let texte = json!({ "profils": [
            { "nom": "Estimateur", "dossier": estimation },
            { "nom": "Perdu", "dossier": d.join("perdu") },
            { "nom": "Vide", "dossier": d },
            { "nom": "ASSISTANTE", "dossier": estimation },
        ]});
        let r = reglages(&d, &texte.to_string());
        assert_eq!(trouver(&r, "Assistante", ""), Ok(None));
        assert_eq!(
            trouver(&r, "Assistante", " assistante "),
            Ok(None),
            "le nom d'office désigne toujours celui d'office"
        );
        let p = trouver(&r, "Assistante", "ESTIMATEUR").unwrap().unwrap();
        assert_eq!((p.nom.as_str(), p.dossier.as_path()), ("Estimateur", estimation.as_path()));
        assert!(trouver(&r, "Assistante", "Perdu").unwrap_err().contains("n'existe pas"));
        assert!(trouver(&r, "Assistante", "Vide").unwrap_err().contains("n'a pas de CLAUDE.md"));
        let e = trouver(&r, "Assistante", "Designer").unwrap_err();
        assert!(e.contains("Aucun profil « Designer »") && e.contains("Assistante (d'office), Estimateur"), "{e}");
        assert_eq!(liste(&r, "Assistante").count(), 3, "celui qui porte le nom d'office n'est pas de la liste");
    }

    #[test]
    fn ajouter_un_profil() {
        let d = dossier("ajouter");
        let (estimation, ailleurs) = (pret(&d, "Estimation"), pret(&d, "Ailleurs"));
        let mut r = reglages(&d, "{\r\n  \"nom\": \"Essai\",\r\n  \"sourdine\": true\r\n}\r\n");
        let m = ajouter(&mut r, "Essai", " Estimateur ", &estimation).unwrap();
        assert!(m.starts_with("Le profil Estimateur est ajouté"), "{m}");
        let lu = relus(&r);
        assert_eq!(lu.profils, [Profil { nom: "Estimateur".into(), dossier: estimation.clone() }]);
        let texte = fs::read_to_string(r.chemin()).unwrap();
        assert!(
            texte.starts_with("{\r\n  \"nom\": \"Essai\",\r\n  \"sourdine\": true,\r\n  \"profils\": ["),
            "{texte}"
        );
        // Le même : rien ne change ; le même nom (sans égard à la casse) prend le nouveau dossier, et sa graphie.
        assert!(ajouter(&mut r, "Essai", "Estimateur", &estimation).unwrap().contains("déjà là"));
        let m = ajouter(&mut r, "Essai", "estimateur", &ailleurs).unwrap();
        assert!(m.contains("prend le dossier") && m.contains("avant"), "{m}");
        assert_eq!(relus(&r).profils, [Profil { nom: "estimateur".into(), dossier: ailleurs.clone() }]);
        // Les refus : le nom d'office, un dossier absent ou sans CLAUDE.md, un nom vide ou trop long.
        let (perdu, long) = (d.join("perdu"), "x".repeat(41));
        for (nom, dossier, attendu) in [
            ("essai", estimation.as_path(), "nom du profil d'office"),
            ("Perdu", perdu.as_path(), "n'existe pas"),
            ("Vide", d.as_path(), "n'a pas de CLAUDE.md"),
            ("  ", estimation.as_path(), "besoin d'un nom"),
            (long.as_str(), estimation.as_path(), "quarante caractères"),
        ] {
            let e = ajouter(&mut r, "Essai", nom, dossier).unwrap_err();
            assert!(e.contains(attendu), "{nom} : {e}");
        }
        assert_eq!(relus(&r).profils.len(), 1, "un refus n'écrit rien");
    }

    /// Un chemin relatif part du dossier courant, et s'inscrit complet.
    #[test]
    fn un_dossier_relatif_s_inscrit_complet() {
        let d = dossier("relatif");
        let courant = std::env::current_dir().unwrap();
        let relatif = chemin_depuis(&pret(&d, "Relatif"), &courant);
        let mut r = reglages(&d, "{}");
        ajouter(&mut r, "Essai", "Relatif", &relatif).unwrap();
        let p = &relus(&r).profils[0];
        assert!(p.dossier.is_absolute() && meme_dossier(&p.dossier, &d.join("Relatif")), "{}", p.dossier.display());
    }

    /// Le chemin de `cible` depuis `depuis`, en remontant autant qu'il faut (les deux sur le même disque).
    fn chemin_depuis(cible: &Path, depuis: &Path) -> PathBuf {
        let (c, f): (Vec<_>, Vec<_>) = (cible.components().collect(), depuis.components().collect());
        let commun = c.iter().zip(&f).take_while(|(a, b)| a == b).count();
        let mut p: PathBuf = f[commun..].iter().map(|_| "..").collect();
        p.extend(&c[commun..]);
        p
    }

    #[test]
    fn choisir_un_profil() {
        let d = dossier("choisir");
        let estimation = pret(&d, "Estimation");
        let texte = json!({ "profils": [
            { "nom": "Estimateur", "dossier": estimation },
            { "nom": "Perdu", "dossier": d.join("perdu") },
        ]});
        let mut r = reglages(&d, &texte.to_string());
        let m = choisir(&mut r, "Essai", "estimateur").unwrap();
        assert!(
            m.starts_with("Le profil Estimateur (") && m.ends_with("est choisi : Eve y passe entre deux demandes.")
        );
        assert_eq!(relus(&r).profil, "Estimateur", "la graphie de la liste");
        assert!(choisir(&mut r, "Essai", "Estimateur").unwrap().ends_with("est déjà le profil choisi."));
        // Un profil qui ne peut pas servir n'est pas choisi ; le choix d'avant reste.
        assert!(choisir(&mut r, "Essai", "Perdu").unwrap_err().contains("n'existe pas"));
        assert!(choisir(&mut r, "Essai", "Designer").unwrap_err().contains("Aucun profil"));
        assert_eq!(relus(&r).profil, "Estimateur");
        // Le nom d'office, ou rien : on y revient.
        assert!(choisir(&mut r, "Essai", "ESSAI").unwrap().starts_with("Le profil d'office, Essai, est choisi"));
        assert_eq!(relus(&r).profil, "");
        assert!(choisir(&mut r, "Essai", "").unwrap().ends_with("est déjà le profil choisi."));
    }

    #[test]
    fn retirer_un_profil() {
        let d = dossier("retirer");
        let (estimation, design) = (pret(&d, "Estimation"), pret(&d, "Design"));
        let texte = json!({
            "profils": [{ "nom": "Estimateur", "dossier": estimation }, { "nom": "Designer", "dossier": design }],
            "profil": "Designer",
        });
        let mut r = reglages(&d, &texte.to_string());
        let m = retirer(&mut r, "Essai", "estimateur").unwrap();
        assert!(m.contains("Le profil Estimateur est retiré") && !m.contains("C'était"), "{m}");
        assert_eq!(relus(&r).profils.len(), 1);
        assert!(estimation.join("CLAUDE.md").is_file(), "son dossier reste tel quel");
        // Le profil choisi : Eve revient à celui d'office.
        assert!(retirer(&mut r, "Essai", "Designer").unwrap().contains("C'était le profil choisi"));
        let lu = relus(&r);
        assert!(lu.profils.is_empty() && lu.profil.is_empty());
        assert!(retirer(&mut r, "Essai", "Essai").unwrap_err().contains("ne se retire pas"));
        assert!(retirer(&mut r, "Essai", "Designer").unwrap_err().contains("Aucun profil « Designer » à retirer"));
    }

    #[test]
    fn lister_les_profils() {
        let d = dossier("lister");
        let estimation = pret(&d, "Estimation");
        let texte = json!({
            "profils": [{ "nom": "Estimateur", "dossier": estimation }, { "nom": "Perdu", "dossier": d.join("perdu") }],
            "profil": "estimateur",
        });
        let r = reglages(&d, &texte.to_string());
        let l = lister(&r, "Essai", Some(Path::new(r"C:\Exemple\Maison")));
        let lignes: Vec<&str> = l.lines().collect();
        assert_eq!(lignes.len(), 4, "{l}");
        assert!(lignes[0].contains("reglages.json"));
        assert_eq!(lignes[1], r"  Essai (d'office) : C:\Exemple\Maison");
        assert_eq!(lignes[2], format!("* Estimateur : {}", estimation.display()));
        assert!(lignes[3].starts_with("  Perdu : ") && lignes[3].ends_with("n'existe pas"), "{}", lignes[3]);
        let seul = reglages(&dossier("lister-seul"), "{}");
        assert!(lister(&seul, "Essai", None).ends_with("\n* Essai (d'office) : le dossier d'Eve"));
    }

    /// Chaque profil garde sa conversation : la séance qu'il laisse en partant, il la retrouve à son retour — dans le
    /// même dossier seulement ; une autre personne devant le poste les oublie toutes.
    #[test]
    fn chaque_profil_garde_sa_conversation() {
        let laisse = |dossier: &str, session: &str, conversation: &str| Laisse {
            dossier: PathBuf::from(dossier),
            session: Some(session.into()),
            conversation: conversation.into(),
        };
        let mut l = Laisses::default();
        let estimation = Path::new(r"C:\Exemple\Estimation");
        let famille = Path::new(r"C:\Exemple\Maison");
        assert_eq!(l.echanger(None, laisse(r"C:\Exemple\Maison", "s1", "c1"), Some("Estimateur"), estimation), None);
        let retour = l.echanger(Some("Estimateur"), laisse(r"C:\Exemple\Estimation", "s2", "c2"), None, famille);
        assert_eq!(retour, Some(laisse(r"C:\Exemple\Maison", "s1", "c1")));
        let encore = l.echanger(
            None,
            laisse(r"C:\Exemple\Maison", "s3", "c3"),
            Some("ESTIMATEUR"),
            Path::new(r"c:\exemple\estimation\"),
        );
        assert_eq!(encore.and_then(|x| x.session).as_deref(), Some("s2"), "sans égard à la casse");
        // Revenu dans un autre dossier : sa séance d'avant n'y vit pas.
        l.echanger(Some("Estimateur"), laisse(r"C:\Exemple\Estimation", "s4", "c4"), None, famille);
        assert_eq!(
            l.echanger(None, laisse(r"C:\Exemple\Maison", "s5", "c5"), Some("Estimateur"), Path::new(r"D:\Ailleurs")),
            None
        );
        l.oublier();
        assert_eq!(l.echanger(Some("Estimateur"), laisse(r"D:\Ailleurs", "s6", "c6"), None, famille), None);
    }

    #[test]
    fn deux_profils_sont_le_meme() {
        let p = |nom: &str, dossier: &str| Profil { nom: nom.into(), dossier: PathBuf::from(dossier) };
        assert!(meme_profil(None, None));
        assert!(meme_profil(Some(&p("Estimateur", r"C:\A")), Some(&p("estimateur", r"c:\a\"))));
        assert!(!meme_profil(Some(&p("Estimateur", r"C:\A")), Some(&p("Estimateur", r"C:\B"))));
        assert!(!meme_profil(Some(&p("Estimateur", r"C:\A")), None));
    }
}
