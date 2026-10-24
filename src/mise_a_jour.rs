//! La mise à jour d'Eve par elle-même, et son retour en arrière (demandée le 24 octobre 2026 : « qu'EVE puisse
//! vraiment modifier son propre code, avec un rollback au cas où »). L'Assistante change son code, le commite, le
//! construit, puis lance `eve --mettre-a-jour <le nouvel eve.exe>` : la version se range à côté du programme, Eve
//! finit sa réponse, dit qu'elle redémarre et se ferme ; son gardien pose la nouvelle, la lance et attend sa marque
//! de santé — sans elle, il remet l'ancienne et la relance. La sécurité vient de ce retour, pas d'un refus : on ne
//! garde que ce qui le rend sûr — une version commitée (on sait où revenir, et `git revert` la défait), une mise à
//! jour à la fois.
//!
//! - **Les versions gardées** : `versions\<empreinte>.exe` à côté du programme (celui de l'Eve qui tourne, sinon ce
//!   programme-ci : jamais un chemin écrit en dur), l'empreinte étant celle que `Compiler.bat` grave dans le
//!   programme ; `versions\versions.json` les dit dans l'ordre où elles ont été posées — la dernière est en place,
//!   celle d'avant est « la précédente » — et en garde cinq. Revenir défait la dernière pose : deux retours de suite
//!   remontent de deux versions, jamais vers celle qu'on vient de quitter.
//! - **`eve --mettre-a-jour <chemin>`** vérifie le fichier (il porte la marque de ce module, son `--version` répond
//!   en moins de 10 s, son empreinte est celle d'un commit sans changement), le range, puis demande à l'Eve qui
//!   tourne (`session.json` de son dossier local) de redémarrer dedans — le geste `mettre_a_jour` ; si aucune ne
//!   tourne, il le pose comme `eve.exe`, simplement.
//! - **L'Eve qui tourne** attend la fin du tour en cours, le dit, lance le gardien (le programme de la version en
//!   place, pris dans `versions\` : `eve.exe` va changer) avec son propre environnement — celui d'`Eve.bat`, jamais
//!   celui d'un shell de Claude Code —, puis se ferme comme au geste « quitter ».
//! - **Le gardien** (`eve --surveiller-la-mise-a-jour …`) attend sa fin, pose la nouvelle d'un geste, la lance et
//!   attend au plus 90 s sa marque de santé (`sante.json` du dossier local : la nouvelle Eve l'écrit quand son chef
//!   tourne depuis cinq secondes et que sa fenêtre a dessiné une image — ni Claude ni le réseau n'y comptent : une
//!   panne d'Internet n'est pas la faute du code). Sans elle, il arrête la nouvelle, remet l'ancienne, le lui dit
//!   (`retour.json`) et la relance. Dans les deux cas, la conversation reprend (la séance de Claude, `--resume`).
//! - **Revenir à la main** : `eve --revenir`, ou le bouton des réglages — la précédente, par le même chemin.
//!
//! Les fichiers qu'échangent les versions (`versions.json`, `verrou.json`, `sante.json`, `mise_a_jour.json`,
//! `retour.json`, le geste, les arguments du gardien) sont un contrat entre elles : une version plus ancienne doit les
//! lire, sans quoi le retour vers elle échouerait. On n'y ajoute que des champs facultatifs.
//!
//! Pour un essai seulement, `EVE_ESSAI_MISE_A_JOUR=<empreinte>:muette` (cette version ne pose jamais sa marque de
//! santé) ou `<empreinte>:arret` (elle s'arrête dès son lancement) : sans la variable, rien ne change.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::journal;
use crate::reglages::{dossier_local, ecrire_d_un_geste};

/// L'argument qui fait d'un programme d'Eve le gardien d'une mise à jour.
pub const ARGUMENT_DU_GARDIEN: &str = "--surveiller-la-mise-a-jour";
/// La marque d'un programme qui sait se mettre à jour : il connaît `--version`, sait être le gardien et pose sa marque
/// de santé. Un programme d'avant ne la porte pas : lancé avec `--version`, il démarrerait toute une Eve, et il ne
/// poserait jamais sa marque — il ne s'installe pas par ce chemin. (Le chiffre : la version du contrat.)
const MARQUE: &str = "[eve:mise-a-jour:1]";
/// Les versions gardées dans `versions\`.
const GARDEES: usize = 5;
/// Le `--version` d'un programme a ce temps pour répondre.
const DELAI_VERSION: Duration = Duration::from_secs(10);
/// La version lancée par le gardien a ce temps pour poser sa marque de santé.
const DELAI_SANTE: Duration = Duration::from_secs(90);
/// La marque se pose quand le chef tourne depuis ce temps, la fenêtre ouverte.
pub const SANTE_APRES: Duration = Duration::from_secs(5);
/// L'Eve qui redémarre a ce temps pour se fermer ; au-delà, son gardien l'arrête.
const DELAI_FERMETURE: Duration = Duration::from_secs(60);
/// Un verrou plus vieux ne tient plus, quel que soit son détenteur (un numéro de processus resservi) : le plus long
/// travail du gardien — la fermeture, la pose, deux attentes de santé — tient en cinq minutes.
const VERROU_MAX: Duration = Duration::from_secs(15 * 60);
/// Un verrou encore vide (il est en train de s'écrire) tient ce temps.
const VERROU_EN_ECRITURE: Duration = Duration::from_secs(10);
/// Au-delà, un passage ne reprend plus la conversation : elle n'est plus « en cours ».
const PASSAGE_FRAIS: Duration = Duration::from_secs(60 * 60);
/// Une version rangée depuis moins longtemps reste dans `versions\`, même hors du registre ([`nettoyer`]).
const RANGEE_RECENTE: Duration = Duration::from_secs(60 * 60);

/// Dans `versions\` : l'ordre des versions posées, et le verrou.
const REGISTRE: &str = "versions.json";
const VERROU: &str = "verrou.json";
/// Dans le dossier local : la marque de santé, et ce que le gardien dit à l'Eve qu'il lance.
const SANTE: &str = "sante.json";
const MISE_A_JOUR: &str = "mise_a_jour.json";
const RETOUR: &str = "retour.json";

// ---- La version de ce programme -------------------------------------------------------------------------------

/// L'empreinte de la source de ce programme (le commit, et « -modifie » si l'arbre avait changé), que pose
/// `Compiler.bat` ; `None` : il a été construit sans elle.
pub fn empreinte() -> Option<&'static str> {
    option_env!("EVE_EMPREINTE").filter(|e| !e.is_empty())
}

/// La révision d'Eve, telle que la fenêtre la montre : « v7 » pour la version 7.0.0 de `Cargo.toml` (sa seule source),
/// « v7.1 » pour la 7.1.0, « v7.1.2 » pour la 7.1.2.
pub fn revision() -> String {
    revision_de(env!("CARGO_PKG_VERSION"))
}

fn revision_de(version: &str) -> String {
    let mut parties: Vec<&str> = version.split('.').collect();
    while parties.len() > 1 && parties.last() == Some(&"0") {
        parties.pop();
    }
    format!("v{}", parties.join("."))
}

/// La version du programme et son empreinte (« 7.0.0 (ad2cca1) ») : notée au journal à chaque démarrage, elle range
/// les mesures par version.
pub fn version() -> String {
    match empreinte() {
        Some(e) => format!("{} ({e})", env!("CARGO_PKG_VERSION")),
        None => env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Ce que dit `eve --version` : « Eve 0.1.0 (ad2cca1) », sur une ligne.
pub fn ligne_de_version() -> String {
    format!("Eve {}", version())
}

/// La version et l'empreinte que dit `eve --version` d'un programme ([`ligne_de_version`]) ; l'empreinte manque s'il a
/// été construit sans elle ; `None` : ce n'est pas la réponse d'un programme d'Eve.
pub fn lire_version(sortie: &str) -> Option<(String, Option<String>)> {
    let reste = sortie.lines().find_map(|l| l.trim().strip_prefix("Eve "))?.trim();
    let (version, empreinte) = match reste.split_once(" (") {
        Some((v, e)) => (v, Some(e.strip_suffix(')')?.trim().to_string())),
        None => (reste, None),
    };
    let version = version.trim();
    (!version.is_empty() && !version.contains(char::is_whitespace)).then(|| (version.to_string(), empreinte))
}

/// Pourquoi un programme d'empreinte `e` ne s'installe pas ; `None` : il s'installe. Seule une version commitée sans
/// changement s'installe : c'est elle qui dit où revenir (son commit, que `git revert` défait).
pub fn refus_d_empreinte(e: Option<&str>) -> Option<String> {
    let Some(e) = e else {
        return Some("il a été construit sans empreinte (sans git, ou pas par Compiler.bat)".into());
    };
    if e.ends_with("-modifie") {
        return Some(format!(
            "il a été construit avec des changements non commités ({e}) : commite d'abord, puis reconstruis"
        ));
    }
    if !empreinte_sure(e) {
        return Some(format!("son empreinte ne se lit pas ({e})"));
    }
    None
}

/// Une empreinte bonne pour un nom de fichier (`versions\<empreinte>.exe`) : celles que rend `git describe`
/// (« ad2cca1 », « v1.0-3-gad2cca1 », « ad2cca1-modifie »).
fn empreinte_sure(e: &str) -> bool {
    !e.is_empty()
        && e.len() <= 64
        && !e.starts_with(['-', '.'])
        && e.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
}

/// Le moyen d'essai qui vise cette version (`EVE_ESSAI_MISE_A_JOUR=<empreinte>:muette`, ou `:arret`), s'il y en a un.
pub fn essai() -> Option<String> {
    let v = std::env::var("EVE_ESSAI_MISE_A_JOUR").ok()?;
    let (e, mode) = v.trim().split_once(':')?;
    (Some(e) == empreinte()).then(|| mode.to_string())
}

// ---- Les versions gardées -------------------------------------------------------------------------------------

/// Vers où va une relance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sorte {
    /// Vers une version neuve (`eve --mettre-a-jour`).
    MiseAJour,
    /// Vers la précédente (`eve --revenir`, le bouton des réglages).
    Retour,
}

/// Une version posée : son empreinte, et quand (« 2026-10-24 14:02:11 »).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Posee {
    pub empreinte: String,
    #[serde(default)]
    pub posee: String,
}

/// `versions\versions.json` : les versions posées, de la plus ancienne à celle en place. Ses fichiers sont les seuls
/// que `versions\` garde.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Registre {
    #[serde(default)]
    pub posees: Vec<Posee>,
}

impl Registre {
    /// Le registre de `versions\` ; absent, vide ; illisible, pourquoi.
    fn lire(versions: &Path) -> Result<Registre, String> {
        let chemin = versions.join(REGISTRE);
        match fs::read_to_string(&chemin) {
            Ok(t) => serde_json::from_str(t.trim_start_matches('\u{feff}'))
                .map_err(|e| format!("{} ne se lit pas ({e})", chemin.display())),
            Err(_) => Ok(Registre::default()),
        }
    }

    /// Le registre, pour le changer : illisible, il repart de zéro (le journal le dit) — les versions rangées restent.
    fn lire_pour_changer(versions: &Path) -> Registre {
        Registre::lire(versions).unwrap_or_else(|e| {
            journal::ecrire(&format!("mise à jour : {e} : il repart de zéro"));
            Registre::default()
        })
    }

    fn ecrire(&self, versions: &Path) -> io::Result<()> {
        let texte = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        ecrire_d_un_geste(&versions.join(REGISTRE), texte.as_bytes())
    }

    /// La version en place.
    pub fn en_place(&self) -> Option<&str> {
        self.posees.last().map(|p| p.empreinte.as_str())
    }

    /// La précédente : celle posée avant la version en place.
    pub fn precedente(&self) -> Option<&str> {
        self.posees.len().checked_sub(2).map(|i| self.posees[i].empreinte.as_str())
    }

    /// Une version de plus, en place ; au-delà de cinq, la plus ancienne s'en va.
    fn poser(&mut self, empreinte: &str, quand: &str) {
        self.posees.push(Posee { empreinte: empreinte.to_string(), posee: quand.to_string() });
        let trop = self.posees.len().saturating_sub(GARDEES);
        self.posees.drain(..trop);
    }

    /// `empreinte` est en place (posée à la main, ou par une version d'avant ce registre) : il le dit.
    fn suivre(&mut self, empreinte: &str, quand: &str) {
        if self.en_place() != Some(empreinte) {
            self.poser(empreinte, quand);
        }
    }

    /// La relance de `de` vers `vers` a réussi : une mise à jour s'ajoute ; un retour défait la dernière pose, pour que
    /// le suivant remonte encore (et ne retourne pas à la version qu'on vient de quitter). `de` manque quand la version
    /// en place était une version d'avant les mises à jour.
    fn apres(&mut self, sorte: Sorte, de: Option<&str>, vers: &str, quand: &str) {
        match sorte {
            Sorte::MiseAJour => {
                if let Some(de) = de {
                    self.suivre(de, quand);
                }
                self.poser(vers, quand);
            }
            Sorte::Retour => {
                if de.is_some() && self.en_place() == de {
                    self.posees.pop();
                }
                self.suivre(vers, quand);
            }
        }
    }

    /// Les fichiers de `versions\` à garder.
    fn fichiers_gardes(&self) -> HashSet<String> {
        self.posees.iter().map(|p| format!("{}.exe", p.empreinte)).collect()
    }
}

/// Le programme qu'une mise à jour remplace (`eve.exe`, celui que lance `Eve.bat`), et ses versions, à côté.
struct Programme {
    exe: PathBuf,
}

impl Programme {
    fn celui_ci() -> Result<Programme, String> {
        std::env::current_exe()
            .map(|exe| Programme { exe })
            .map_err(|e| format!("le chemin du programme est introuvable ({e})"))
    }

    fn versions(&self) -> PathBuf {
        self.exe.with_file_name("versions")
    }

    fn fichier(&self, empreinte: &str) -> PathBuf {
        self.versions().join(format!("{empreinte}.exe"))
    }
}

/// Le programme porte-t-il la marque d'un programme qui sait se mettre à jour ([`MARQUE`]) ? La marque passe par
/// `black_box` : le compilateur ne peut pas en faire des comparaisons d'entiers, ses octets restent tels quels dans le
/// programme — et dans tout programme construit avec ce module, qui la cherche.
fn porte_la_marque(exe: &Path) -> io::Result<bool> {
    let octets = fs::read(exe)?;
    let aiguille: &[u8] = std::hint::black_box(MARQUE.as_bytes());
    Ok(octets.windows(aiguille.len()).any(|w| w[0] == aiguille[0] && w == aiguille))
}

/// La version et l'empreinte d'un programme, d'après son `--version` (10 s au plus) — s'il porte la marque : un
/// programme d'avant ferait de `--version` le lancement de toute une Eve.
fn demander_la_version(exe: &Path) -> Result<(String, Option<String>), String> {
    match porte_la_marque(exe) {
        Ok(true) => {}
        Ok(false) => {
            return Err(format!(
                "{} ne sait pas se mettre à jour (un programme d'avant) : il ne poserait jamais sa marque de santé",
                exe.display()
            ));
        }
        Err(e) => return Err(format!("{} ne se lit pas ({e})", exe.display())),
    }
    let mut c = Command::new(exe);
    c.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    crate::cerveau::sans_fenetre(&mut c);
    let mut enfant = c.spawn().map_err(|e| format!("{} ne se lance pas ({e})", exe.display()))?;
    let fin = Instant::now() + DELAI_VERSION;
    loop {
        match enfant.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < fin => thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = enfant.kill();
                let _ = enfant.wait();
                return Err(format!("{} n'a pas dit sa version en 10 s", exe.display()));
            }
        }
    }
    let mut sortie = String::new();
    if let Some(mut s) = enfant.stdout.take() {
        let _ = s.read_to_string(&mut sortie);
    }
    lire_version(&sortie).ok_or_else(|| format!("{} ne dit pas sa version (« {} »)", exe.display(), sortie.trim()))
}

/// Range un programme dans `versions\` (une version déjà rangée y reste telle quelle).
fn ranger(source: &Path, cible: &Path) -> io::Result<()> {
    if cible.is_file() {
        return Ok(());
    }
    if let Some(d) = cible.parent() {
        fs::create_dir_all(d)?;
    }
    ecrire_d_un_geste(cible, &fs::read(source)?)
}

/// Pose `source` comme `programme`, d'un geste : la copie à côté, puis un renommage. Un programme en marche (une autre
/// Eve du poste le fait tourner) ne se remplace pas, mais se renomme : il passe d'abord à côté
/// (`eve.remplace.<date>.exe`, que retire `Eve.bat`), comme le fait l'installateur.
fn poser(source: &Path, programme: &Path) -> io::Result<()> {
    let octets = fs::read(source)?;
    let premiere = match ecrire_d_un_geste(programme, &octets) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    if !programme.exists() {
        return Err(premiere);
    }
    let tige = programme.file_stem().map(|t| t.to_string_lossy().into_owned()).unwrap_or_else(|| "eve".into());
    let date = journal::horodatage().replace('-', "").replace(' ', "-");
    let a_cote = programme.with_file_name(format!("{tige}.remplace.{date}.exe"));
    fs::rename(programme, &a_cote).map_err(|_| premiere)?;
    ecrire_d_un_geste(programme, &octets).inspect_err(|_| {
        let _ = fs::rename(&a_cote, programme);
    })
}

/// Retire de `versions\` ce que le registre ne garde plus (une version tombée des cinq, une version qui n'a pas
/// démarré, une copie interrompue). Une version rangée depuis moins d'une heure reste : une relance l'attend
/// peut-être encore (elle n'entre au registre qu'une fois debout). Un fichier encore ouvert reste aussi : il partira
/// la fois suivante.
fn nettoyer(p: &Programme, registre: &Registre) {
    let gardes = registre.fichiers_gardes();
    let recent = |f: &fs::DirEntry| {
        f.metadata().and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).is_none_or(|a| a < RANGEE_RECENTE)
    };
    for f in fs::read_dir(p.versions()).into_iter().flatten().flatten() {
        let nom = f.file_name().to_string_lossy().into_owned();
        let de_trop = (nom.ends_with(".exe") && !gardes.contains(&nom) && !recent(&f)) || nom.ends_with(".tmp");
        if de_trop && fs::remove_file(f.path()).is_ok() {
            journal::ecrire(&format!("mise à jour : versions\\{nom} retiré"));
        }
    }
}

/// La version en place : celle que dit `eve.exe` ; `None` : pas de programme, ou une version d'avant les mises à jour.
/// Le registre suit : une version posée à la main y entre, rangée dans `versions\`.
fn version_en_place(p: &Programme, registre: &mut Registre, quand: &str) -> Result<Option<String>, String> {
    if !p.exe.is_file() || !porte_la_marque(&p.exe).unwrap_or(false) {
        return Ok(None);
    }
    let (_, e) = demander_la_version(&p.exe)?;
    let Some(e) = e.filter(|e| empreinte_sure(e)) else { return Ok(None) };
    ranger(&p.exe, &p.fichier(&e)).map_err(|err| format!("la version en place ne se range pas ({err})"))?;
    if registre.en_place() != Some(e.as_str()) {
        registre.suivre(&e, quand);
        registre.ecrire(&p.versions()).map_err(|err| format!("{REGISTRE} ne s'écrit pas ({err})"))?;
    }
    Ok(Some(e))
}

// ---- Le verrou ------------------------------------------------------------------------------------------------

/// Ce que dit `versions\verrou.json` : qui le tient (son processus), depuis quand (en secondes Unix), pour quoi.
#[derive(Serialize, Deserialize)]
struct Tenue {
    pid: u32,
    depuis: u64,
    quoi: String,
}

/// Pourquoi une relance ne part pas.
#[derive(Debug)]
pub enum Empeche {
    /// Une autre mise à jour est en cours : celle-ci attendra.
    Occupe(String),
    Echec(String),
}

impl std::fmt::Display for Empeche {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Empeche::Occupe(s) | Empeche::Echec(s) => f.write_str(s),
        }
    }
}

/// Le verrou des mises à jour : une seule à la fois (le registre, `versions\`, `eve.exe`). Il tient tant que son
/// détenteur vit, quinze minutes au plus ; il se lève quand ce programme-ci le lâche, sauf s'il l'a confié au gardien.
struct Verrou {
    chemin: PathBuf,
}

impl Verrou {
    fn prendre(versions: &Path, quoi: &str) -> Result<Verrou, Empeche> {
        let chemin = versions.join(VERROU);
        fs::create_dir_all(versions)
            .map_err(|e| Empeche::Echec(format!("{} ne se crée pas ({e})", versions.display())))?;
        for _ in 0..2 {
            match fs::OpenOptions::new().write(true).create_new(true).open(&chemin) {
                Ok(mut f) => {
                    let t = Tenue { pid: std::process::id(), depuis: secondes(), quoi: quoi.to_string() };
                    let _ = f.write_all(serde_json::to_string(&t).unwrap_or_default().as_bytes());
                    let _ = f.sync_all();
                    return Ok(Verrou { chemin });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    if let Some(occupe) = tenu(&chemin) {
                        return Err(Empeche::Occupe(format!("une autre mise à jour est en cours ({occupe})")));
                    }
                    journal::ecrire("mise à jour : un verrou abandonné est levé");
                    let _ = fs::remove_file(&chemin);
                }
                Err(e) => return Err(Empeche::Echec(format!("le verrou ne se pose pas ({e})"))),
            }
        }
        Err(Empeche::Occupe("le verrou vient d'être pris par une autre mise à jour".into()))
    }

    /// Le verrou passe au gardien (`pid`) : il ne se lève plus quand ce programme-ci s'arrête.
    fn confier(self, pid: u32) {
        let quoi = fs::read_to_string(&self.chemin)
            .ok()
            .and_then(|t| serde_json::from_str::<Tenue>(&t).ok())
            .map(|t| t.quoi)
            .unwrap_or_default();
        let t = Tenue { pid, depuis: secondes(), quoi };
        let _ = ecrire_d_un_geste(&self.chemin, serde_json::to_string(&t).unwrap_or_default().as_bytes());
        std::mem::forget(self);
    }
}

impl Drop for Verrou {
    fn drop(&mut self) {
        lever(&self.chemin);
    }
}

/// Lève le verrou s'il est à ce programme-ci.
fn lever(chemin: &Path) {
    let a_moi = fs::read_to_string(chemin)
        .ok()
        .and_then(|t| serde_json::from_str::<Tenue>(&t).ok())
        .is_some_and(|t| t.pid == std::process::id());
    if a_moi {
        let _ = fs::remove_file(chemin);
    }
}

/// Le verrou tient-il encore ? Ce qu'il retient, s'il tient ; `None` : abandonné (son détenteur est mort, ou il a plus
/// de quinze minutes).
fn tenu(chemin: &Path) -> Option<String> {
    let age = fs::metadata(chemin).and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).unwrap_or_default();
    match fs::read_to_string(chemin).ok().and_then(|t| serde_json::from_str::<Tenue>(&t).ok()) {
        Some(t) => {
            let depuis = Duration::from_secs(secondes().saturating_sub(t.depuis));
            (vivant(t.pid) && depuis < VERROU_MAX).then_some(t.quoi)
        }
        None => (age < VERROU_EN_ECRITURE).then(|| "il est en train de se poser".to_string()),
    }
}

fn secondes() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---- Les processus --------------------------------------------------------------------------------------------

#[cfg(windows)]
mod processus {
    use std::time::Duration;

    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::path::PathBuf;

    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
    };
    use windows::core::PWSTR;

    /// Un processus d'un autre programme, par son numéro : on peut attendre sa fin. Tant que la poignée vit, son numéro
    /// ne sert à personne d'autre.
    pub struct Processus(HANDLE);

    impl Processus {
        pub fn ouvrir(pid: u32) -> Option<Processus> {
            // SAFETY : une poignée de synchronisation sur un processus ; refusée (il n'existe plus), aucun effet.
            unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.ok().map(Processus)
        }

        /// Attend sa fin, `delai` au plus : vrai s'il est fini (ou ne s'attend pas).
        pub fn attendre(&self, delai: Duration) -> bool {
            let ms = u32::try_from(delai.as_millis()).unwrap_or(u32::MAX - 1);
            // SAFETY : la poignée vit autant que `self`.
            let attente = unsafe { WaitForSingleObject(self.0, ms) };
            attente != WAIT_TIMEOUT
        }
    }

    impl Drop for Processus {
        fn drop(&mut self) {
            // SAFETY : la poignée vient d'OpenProcess et ne sert plus.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    /// Arrête le processus `pid` (l'Eve qui devait se fermer et ne se ferme pas).
    pub fn arreter(pid: u32) {
        // SAFETY : une poignée d'arrêt, servie puis fermée ; refusée, aucun effet.
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(h, 1);
                let _ = CloseHandle(h);
            }
        }
    }

    /// Le programme que fait tourner le processus `pid` (son image).
    pub fn image(pid: u32) -> Option<PathBuf> {
        let mut tampon = vec![0u16; 32_768];
        let mut n = tampon.len() as u32;
        // SAFETY : une poignée de lecture, servie puis fermée ; le tampon et sa taille vivent le temps de l'appel.
        let lu = unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let lu = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(tampon.as_mut_ptr()), &mut n);
            let _ = CloseHandle(h);
            lu
        };
        lu.ok()?;
        tampon.truncate(n as usize);
        Some(PathBuf::from(OsString::from_wide(&tampon)))
    }
}

#[cfg(not(windows))]
mod processus {
    use std::path::PathBuf;
    use std::time::Duration;

    pub struct Processus;

    impl Processus {
        pub fn ouvrir(_pid: u32) -> Option<Processus> {
            None
        }

        pub fn attendre(&self, _delai: Duration) -> bool {
            true
        }
    }

    pub fn arreter(_pid: u32) {}

    pub fn image(_pid: u32) -> Option<PathBuf> {
        None
    }
}

/// Le processus `pid` vit-il ?
fn vivant(pid: u32) -> bool {
    pid != 0 && processus::Processus::ouvrir(pid).is_some_and(|p| !p.attendre(Duration::ZERO))
}

/// Un programme d'Eve, par son nom : celui que lance `Eve.bat`.
fn nomme_eve(exe: &Path) -> bool {
    exe.file_name().is_some_and(|n| n.eq_ignore_ascii_case("eve.exe"))
}

/// Un programme d'Eve lancé à part : sans console (sa sortie n'a nulle part où aller : son journal est `eve.log`),
/// dans son propre groupe de console (un Ctrl+C ne le touche pas), et hors du groupe de tâches de celui qui le lance
/// quand il est permis d'en sortir — le gardien survit à l'Eve qui se ferme, la nouvelle Eve au gardien.
#[cfg(windows)]
fn lancer_a_part(mut c: Command) -> io::Result<Child> {
    use std::os::windows::process::CommandExt;

    use windows::Win32::System::Threading::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let a_part = CREATE_NO_WINDOW.0 | CREATE_NEW_PROCESS_GROUP.0;
    c.creation_flags(a_part | CREATE_BREAKAWAY_FROM_JOB.0);
    match c.spawn() {
        // Un groupe de tâches qui ne laisse pas sortir : l'enfant y reste.
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            c.creation_flags(a_part);
            c.spawn()
        }
        r => r,
    }
}

#[cfg(not(windows))]
fn lancer_a_part(mut c: Command) -> io::Result<Child> {
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    c.spawn()
}

// ---- La marque de santé et les passages ---------------------------------------------------------------------

/// Ce que dit `sante.json`.
#[derive(Serialize, Deserialize)]
struct Sante {
    empreinte: String,
    pid: u32,
    #[serde(default)]
    heure: String,
}

/// La marque de santé dit-elle que l'Eve d'empreinte `empreinte`, lancée sous le numéro `pid`, est debout ? (Une marque
/// d'avant, celle d'une autre version ou d'un autre lancement, ne compte pas.)
fn sante_valable(texte: &str, empreinte: &str, pid: u32) -> bool {
    serde_json::from_str::<Sante>(texte).is_ok_and(|s| s.empreinte == empreinte && s.pid == pid)
}

/// Pose la marque de santé de cette Eve : son chef tourne et sa fenêtre est ouverte (`apres` : depuis le démarrage). Le
/// gardien d'une mise à jour l'attend pour garder la nouvelle version.
pub fn poser_la_marque(apres: Duration) {
    if essai().as_deref() == Some("muette") {
        journal::ecrire("essai : cette version ne pose pas sa marque de santé (EVE_ESSAI_MISE_A_JOUR)");
        return;
    }
    let s = Sante {
        empreinte: empreinte().unwrap_or("").to_string(),
        pid: std::process::id(),
        heure: journal::date_et_heure(),
    };
    let texte = serde_json::to_string(&s).unwrap_or_default();
    match ecrire_d_un_geste(&dossier_local().join(SANTE), texte.as_bytes()) {
        Ok(()) => journal::ecrire(&format!("marque de santé posée ({:.1} s après le démarrage)", apres.as_secs_f64())),
        Err(e) => journal::ecrire(&format!("la marque de santé ne s'écrit pas ({e})")),
    }
}

/// Ce que le gardien dit à l'Eve qu'il lance : `mise_a_jour.json` (« tu es la version `vers`, tu remplaces `de` ») ou
/// `retour.json` (« `de` n'a pas démarré, pour `raison` : tu es revenue, toi, `vers` ») ; et la séance de Claude à
/// reprendre.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Passage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sorte: Option<Sorte>,
    de: String,
    vers: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    raison: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<String>,
    #[serde(default)]
    heure: String,
}

fn ecrire_passage(chemin: &Path, p: &Passage) {
    let texte = serde_json::to_string_pretty(p).unwrap_or_default();
    if let Err(e) = ecrire_d_un_geste(chemin, texte.as_bytes()) {
        journal::ecrire(&format!("mise à jour : {} ne s'écrit pas ({e})", chemin.display()));
    }
}

/// Lit un passage et le retire (il ne vaut qu'une fois) ; et s'il est frais (moins d'une heure).
fn prendre_le_passage(chemin: &Path) -> Option<(Passage, bool)> {
    let texte = fs::read_to_string(chemin).ok()?;
    let frais = fs::metadata(chemin)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|m| m.elapsed().ok())
        .is_some_and(|a| a < PASSAGE_FRAIS);
    let _ = fs::remove_file(chemin);
    serde_json::from_str(texte.trim_start_matches('\u{feff}')).ok().map(|p| (p, frais))
}

/// Ce qu'une Eve apprend de la relance qui l'a lancée.
#[derive(Debug, Default)]
pub struct Depart {
    /// La conversation à reprendre (`--resume`) : celle de l'Eve d'avant la relance.
    pub session: Option<String>,
    /// Écrit au fil et dit dès le démarrage (le fil, la voix) : la nouvelle version n'a pas démarré.
    pub annonce: Option<(String, String)>,
    /// Écrit et dit quand la marque de santé est posée : la relance a réussi.
    pub a_la_sante: Option<(String, String)>,
}

/// Au démarrage : ce que le gardien a laissé dans le dossier local pour cette Eve (lu, puis retiré).
pub fn au_demarrage() -> Depart {
    depart_dans(&dossier_local(), empreinte().unwrap_or(""))
}

fn depart_dans(local: &Path, moi: &str) -> Depart {
    let mut d = Depart::default();
    let retour = prendre_le_passage(&local.join(RETOUR));
    let relance = prendre_le_passage(&local.join(MISE_A_JOUR));
    if let Some((r, frais)) = retour {
        journal::ecrire(&format!(
            "mise à jour : la version {} n'a pas démarré — {} ; cette Eve est revenue à {}",
            r.de, r.raison, r.vers
        ));
        d.annonce = Some((
            format!(
                "Ma nouvelle version ({}) n'a pas démarré : {}. Je suis revenue à la précédente ({}) ; le détail \
                 est au journal (eve.log).",
                r.de, r.raison, r.vers
            ),
            "Ma nouvelle version n'a pas démarré : je suis revenue à la précédente.".into(),
        ));
        if frais {
            d.session = r.session;
        }
    } else if let Some((m, frais)) = relance
        && m.vers == moi
        && frais
    {
        let reprise = if m.session.is_some() { " ; la conversation reprend" } else { "" };
        journal::ecrire(&format!("mise à jour : cette Eve est la version {} (avant : {}){reprise}", m.vers, m.de));
        d.a_la_sante = Some(match m.sorte.unwrap_or(Sorte::MiseAJour) {
            Sorte::MiseAJour => (
                format!("Mise à jour faite : je suis maintenant à la version {} (avant : {}).", m.vers, m.de),
                "Me voici dans ma nouvelle version.".into(),
            ),
            Sorte::Retour => (
                format!("Retour fait : je suis revenue à la version {} (je quitte {}).", m.vers, m.de),
                "Je suis revenue à ma version précédente.".into(),
            ),
        });
        d.session = m.session;
    }
    d
}

// ---- La relance, dans l'Eve qui tourne ------------------------------------------------------------------------

/// Une relance demandée à l'Eve qui tourne : la version où redémarrer, celle qui est en place (le gardien en vient, et
/// c'est elle qui revient si l'autre ne démarre pas), et la sorte. C'est aussi le geste `mettre_a_jour`
/// (`{"vers": …, "de": …, "sorte": "mise_a_jour"}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Relance {
    pub vers: String,
    pub de: String,
    pub sorte: Sorte,
}

impl Relance {
    /// La relance que demande le geste `mettre_a_jour`, si ses deux versions sont rangées à côté de ce programme.
    pub fn du_geste(valeur: &Value) -> Result<Relance, String> {
        let r: Relance = serde_json::from_value(valeur.clone()).map_err(|e| format!("le geste ne se lit pas ({e})"))?;
        r.verifier(&Programme::celui_ci()?)?;
        Ok(r)
    }

    /// Le retour à la précédente (le bouton des réglages) : celle posée avant cette version, au registre.
    pub fn vers_la_precedente() -> Result<Relance, String> {
        let p = Programme::celui_ci()?;
        let moi = empreinte().ok_or("ce programme a été construit sans empreinte")?;
        let registre = Registre::lire(&p.versions())?;
        if registre.en_place() != Some(moi) {
            return Err(format!("le registre des versions ne dit pas cette version en place ({moi})"));
        }
        let vers = registre.precedente().ok_or("aucune version précédente n'est gardée")?;
        let r = Relance { vers: vers.to_string(), de: moi.to_string(), sorte: Sorte::Retour };
        r.verifier(&p)?;
        Ok(r)
    }

    fn verifier(&self, p: &Programme) -> Result<(), String> {
        if self.vers == self.de {
            return Err(format!("la version {} est déjà en place", self.vers));
        }
        for e in [&self.vers, &self.de] {
            if !empreinte_sure(e) {
                return Err(format!("l'empreinte « {e} » ne se lit pas"));
            }
            let f = p.fichier(e);
            if !porte_la_marque(&f).unwrap_or(false) {
                return Err(format!("{} manque, ou ne sait pas se mettre à jour", f.display()));
            }
        }
        Ok(())
    }

    /// Ce qu'elle est, pour le journal et le verrou.
    pub fn quoi(&self) -> String {
        match self.sorte {
            Sorte::MiseAJour => format!("mise à jour de {} vers {}", self.de, self.vers),
            Sorte::Retour => format!("retour de {} à {}", self.de, self.vers),
        }
    }

    /// Ce qu'Eve écrit au fil, et ce qu'elle dit, avant de redémarrer.
    pub fn annonce(&self) -> (String, &'static str) {
        match self.sorte {
            Sorte::MiseAJour => (
                format!(
                    "Je redémarre avec ma nouvelle version ({}) ; la conversation reprendra où elle en est.",
                    self.vers
                ),
                "Je redémarre avec ma nouvelle version.",
            ),
            Sorte::Retour => (
                format!(
                    "Je reviens à ma version précédente ({}) ; la conversation reprendra où elle en est.",
                    self.vers
                ),
                "Je reviens à ma version précédente.",
            ),
        }
    }
}

/// La version précédente que les réglages offrent : celle d'avant cette version, si le registre la dit en place et si
/// son programme est là.
pub fn precedente_offerte() -> Option<String> {
    let p = Programme::celui_ci().ok()?;
    let registre = Registre::lire(&p.versions()).ok()?;
    if registre.en_place()? != empreinte()? {
        return None;
    }
    let vers = registre.precedente()?;
    p.fichier(vers).is_file().then(|| vers.to_string())
}

/// Les arguments du gardien : `<pid> <vers> <de> --programme <eve.exe> [--retour] [--session <id>] -- <ceux d'Eve>`.
fn arguments_du_gardien(r: &Relance, pid: u32, session: Option<&str>, programme: &Path, eve: &[String]) -> Vec<String> {
    let mut a = vec![ARGUMENT_DU_GARDIEN.to_string(), pid.to_string(), r.vers.clone(), r.de.clone()];
    a.extend(["--programme".to_string(), programme.to_string_lossy().into_owned()]);
    if r.sorte == Sorte::Retour {
        a.push("--retour".into());
    }
    if let Some(s) = session {
        a.extend(["--session".to_string(), s.to_string()]);
    }
    a.push("--".into());
    a.extend(eve.iter().cloned());
    a
}

/// L'Eve qui tourne lance le gardien de sa relance — le programme de la version en place, dans `versions\` (`eve.exe`
/// va changer) — avec son environnement à elle (celui d'`Eve.bat`), son dossier courant et ses arguments ; puis elle se
/// ferme. `session` : la séance de Claude, que la version suivante reprendra. Rend le numéro du gardien.
pub fn lancer_le_gardien(r: &Relance, session: Option<&str>) -> Result<u32, Empeche> {
    let p = Programme::celui_ci().map_err(Empeche::Echec)?;
    r.verifier(&p).map_err(Empeche::Echec)?;
    let verrou = Verrou::prendre(&p.versions(), &r.quoi())?;
    let eve: Vec<String> = std::env::args().skip(1).collect();
    let mut c = Command::new(p.fichier(&r.de));
    c.args(arguments_du_gardien(r, std::process::id(), session, &p.exe, &eve));
    // Le premier lancement après l'installation (qui relie Eve à Constructo AI) ne se refait pas à chaque relance.
    c.env_remove("EVE_RELIER_CONSTRUCTO");
    let enfant = lancer_a_part(c).map_err(|e| Empeche::Echec(format!("le gardien ne se lance pas ({e})")))?;
    let pid = enfant.id();
    verrou.confier(pid);
    Ok(pid)
}

// ---- Les commandes --------------------------------------------------------------------------------------------

/// `eve --mettre-a-jour <chemin du nouvel eve.exe>` : voir le haut de ce fichier. Rend le code de sortie.
pub fn mettre_a_jour(args: &[String]) -> i32 {
    commencer();
    let Some(chemin) = args.first().filter(|c| !c.trim().is_empty()) else {
        eprintln!("eve --mettre-a-jour <chemin du nouvel eve.exe>");
        return 2;
    };
    let source = PathBuf::from(chemin);
    let fait = (|| {
        if !source.is_file() {
            return Err(format!("{} n'existe pas", source.display()));
        }
        let (_, e) = demander_la_version(&source)?;
        if let Some(r) = refus_d_empreinte(e.as_deref()) {
            return Err(format!("{} ne s'installe pas : {r}", source.display()));
        }
        executer(Sorte::MiseAJour, Some((&source, e.as_deref().unwrap_or_default())))
    })();
    finir(fait)
}

/// `eve --revenir` : la version précédente, par le même chemin qu'une mise à jour.
pub fn revenir(_args: &[String]) -> i32 {
    commencer();
    finir(executer(Sorte::Retour, None))
}

fn commencer() {
    journal::sans_console();
    journal::ouvrir(&dossier_local());
}

fn finir(fait: Result<String, String>) -> i32 {
    match fait {
        Ok(ligne) => {
            journal::ecrire(&format!("mise à jour : {ligne}"));
            println!("{ligne}");
            0
        }
        Err(e) => {
            journal::ecrire(&format!("mise à jour refusée : {e}"));
            eprintln!("Mise à jour refusée : {e}.");
            1
        }
    }
}

/// Une mise à jour (`nouvelle` : le fichier et son empreinte) ou un retour à la précédente, sous le verrou : l'Eve qui
/// tourne redémarre dedans à la fin de sa réponse ; si aucune ne tourne (ou une version d'avant, qui ne sait pas
/// redémarrer), la version se pose simplement.
fn executer(sorte: Sorte, nouvelle: Option<(&Path, &str)>) -> Result<String, String> {
    // L'Eve qui tourne avec ce dossier local, et son programme : l'image de son processus — celle d'une version d'avant
    // aussi. Un numéro de processus resservi à un autre programme ne compte pas : seul un `eve.exe` se remplace.
    let en_marche = crate::ecran::session_notee()
        .filter(|s| s.pid != std::process::id() && vivant(s.pid))
        .and_then(|s| processus::image(s.pid).filter(|i| nomme_eve(i)).map(|exe| (s, exe)));
    let (eve, p) = match en_marche {
        Some((s, exe)) => (Some(s), Programme { exe }),
        None => (None, Programme::celui_ci()?),
    };
    let versions = p.versions();
    let quoi = match nouvelle {
        Some((_, vers)) => format!("mise à jour vers {vers}"),
        None => "retour à la version précédente".to_string(),
    };
    let verrou = Verrou::prendre(&versions, &quoi).map_err(|e| e.to_string())?;
    let quand = journal::date_et_heure();
    let mut registre = Registre::lire_pour_changer(&versions);
    let de = version_en_place(&p, &mut registre, &quand)?;
    nettoyer(&p, &registre);
    let vers = match nouvelle {
        Some((source, vers)) => {
            if de.as_deref() == Some(vers) {
                return Ok(format!("La version {vers} est déjà en place ({}) : rien à faire.", p.exe.display()));
            }
            ranger(source, &p.fichier(vers)).map_err(|e| format!("{} ne se range pas ({e})", source.display()))?;
            vers.to_string()
        }
        None => {
            // Une version d'avant en place : on revient à la dernière que le registre a posée.
            let cible = if de.is_some() { registre.precedente() } else { registre.en_place() };
            let vers = cible.ok_or("aucune version précédente n'est gardée")?.to_string();
            let f = p.fichier(&vers);
            if !porte_la_marque(&f).unwrap_or(false) {
                return Err(format!("{} manque, ou ne sait pas se mettre à jour", f.display()));
            }
            vers
        }
    };
    if let (Some(s), Some(de)) = (&eve, &de)
        && s.mise_a_jour >= 1
    {
        let geste = json!(Relance { vers: vers.clone(), de: de.clone(), sorte });
        match crate::ecran::geste_a_l_eve(s.port, &s.jeton, "mettre_a_jour", &geste) {
            Ok(()) => {
                return Ok(match sorte {
                    Sorte::MiseAJour => format!(
                        "Eve (processus {}) redémarrera dans la version {vers} à la fin de sa réponse en cours ; si \
                         celle-ci ne démarre pas, elle reviendra d'elle-même à {de}.",
                        s.pid
                    ),
                    Sorte::Retour => format!(
                        "Eve (processus {}) reviendra à la version précédente, {vers}, à la fin de sa réponse en cours \
                         ; si celle-ci ne démarre pas, elle restera à {de}.",
                        s.pid
                    ),
                });
            }
            // Plus personne n'écoute sur son port : comme si aucune ne tournait.
            Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => {}
            Err(e) => return Err(format!("l'Eve qui tourne (processus {}) ne reçoit pas la demande ({e})", s.pid)),
        }
    }
    poser(&p.fichier(&vers), &p.exe).map_err(|e| format!("{} ne se pose pas ({e})", p.exe.display()))?;
    registre.apres(sorte, de.as_deref(), &vers, &quand);
    if let Err(e) = registre.ecrire(&versions) {
        journal::ecrire(&format!("mise à jour : {REGISTRE} ne s'écrit pas ({e})"));
    }
    drop(verrou);
    let a_la_place = de.map_or_else(|| "d'une version d'avant les mises à jour".to_string(), |d| format!("de {d}"));
    Ok(match eve {
        Some(s) => format!(
            "L'Eve qui tourne (processus {}) ne sait pas redémarrer d'elle-même (une version d'avant) : la version \
             {vers} est posée à la place {a_la_place} ({}) ; elle vaudra à sa prochaine relance.",
            s.pid,
            p.exe.display()
        ),
        None => format!(
            "Aucune Eve ne tourne : la version {vers} est posée à la place {a_la_place} ({}) ; elle vaudra au \
             prochain lancement.",
            p.exe.display()
        ),
    })
}

// ---- Le gardien -----------------------------------------------------------------------------------------------

/// Le gardien d'une relance, d'après ses arguments ([`arguments_du_gardien`]).
#[derive(Debug, PartialEq)]
struct Gardien {
    /// L'Eve qui se ferme.
    pid: u32,
    relance: Relance,
    session: Option<String>,
    programme: PathBuf,
    /// Les arguments de l'Eve, pour la relancer pareille.
    eve: Vec<String>,
}

impl Gardien {
    fn lire(args: &[String]) -> Result<Gardien, String> {
        let mut it = args.iter();
        let pid = it.next().and_then(|p| p.parse().ok()).ok_or("le numéro de l'Eve manque")?;
        let mut version_dite =
            |quoi: &str| it.next().filter(|e| empreinte_sure(e)).cloned().ok_or_else(|| format!("{quoi} manque"));
        let vers = version_dite("la version à poser")?;
        let de = version_dite("la version en place")?;
        let (mut sorte, mut session, mut programme, mut eve) = (Sorte::MiseAJour, None, None, Vec::new());
        while let Some(a) = it.next() {
            match a.as_str() {
                "--retour" => sorte = Sorte::Retour,
                "--session" => session = it.next().cloned(),
                "--programme" => programme = it.next().map(PathBuf::from),
                "--" => {
                    eve = it.by_ref().cloned().collect();
                    break;
                }
                autre => return Err(format!("l'argument « {autre} » est inconnu")),
            }
        }
        let programme = programme.ok_or("le programme à remplacer manque")?;
        Ok(Gardien { pid, relance: Relance { vers, de, sorte }, session, programme, eve })
    }

    fn p(&self) -> Programme {
        Programme { exe: self.programme.clone() }
    }

    /// Le travail du gardien : voir le haut de ce fichier. Rend le code de sortie (0 : la nouvelle version est en place
    /// et répond ; 1 : l'ancienne est revenue).
    fn mener(&self) -> i32 {
        let (r, p, local) = (&self.relance, self.p(), dossier_local());
        journal::ecrire(&format!("mise à jour : le gardien attend la fermeture de l'Eve {} ({})", self.pid, r.quoi()));
        if let Some(ancienne) = processus::Processus::ouvrir(self.pid)
            && !ancienne.attendre(DELAI_FERMETURE)
        {
            journal::ecrire("mise à jour : l'Eve ne s'est pas fermée en une minute ; le gardien l'arrête");
            processus::arreter(self.pid);
            ancienne.attendre(Duration::from_secs(10));
        }
        if let Err(e) = poser(&p.fichier(&r.vers), &p.exe) {
            return self.revenir(&format!("elle ne s'est pas posée ({e})"), false);
        }
        journal::ecrire(&format!("mise à jour : {} est posée ; le gardien la lance", r.vers));
        let passage = Passage {
            sorte: Some(r.sorte),
            de: r.de.clone(),
            vers: r.vers.clone(),
            session: self.session.clone(),
            heure: journal::date_et_heure(),
            ..Passage::default()
        };
        ecrire_passage(&local.join(MISE_A_JOUR), &passage);
        let raison = match self.lancer() {
            Err(e) => format!("elle ne s'est pas lancée ({e})"),
            Ok(mut nouvelle) => match attendre_la_sante(&local, &r.vers, &mut nouvelle) {
                Issue::Bonne(t) => {
                    journal::ecrire(&format!(
                        "mise à jour : {} est en place et répond (sa marque de santé en {:.1} s)",
                        r.vers,
                        t.as_secs_f64()
                    ));
                    let mut registre = Registre::lire_pour_changer(&p.versions());
                    registre.apres(r.sorte, Some(&r.de), &r.vers, &journal::date_et_heure());
                    if let Err(e) = registre.ecrire(&p.versions()) {
                        journal::ecrire(&format!("mise à jour : {REGISTRE} ne s'écrit pas ({e})"));
                    }
                    lever(&p.versions().join(VERROU));
                    return 0;
                }
                Issue::Arretee(code) => {
                    let code = code.map_or_else(|| "sans code".to_string(), |c| format!("code {c}"));
                    format!("elle s'est arrêtée avant de poser sa marque de santé ({code})")
                }
                Issue::Muette => {
                    let _ = nouvelle.kill();
                    let _ = nouvelle.wait();
                    format!("elle n'a pas posé sa marque de santé en {} s", DELAI_SANTE.as_secs())
                }
            },
        };
        self.revenir(&raison, true)
    }

    /// La nouvelle version n'a pas démarré (`raison`) : l'ancienne revient (si la nouvelle était `posee`, elle se
    /// repose), le sait (`retour.json`) et se relance.
    fn revenir(&self, raison: &str, posee: bool) -> i32 {
        let (r, p, local) = (&self.relance, self.p(), dossier_local());
        journal::ecrire(&format!("mise à jour : {} n'a pas démarré — {raison} ; le gardien remet {}", r.vers, r.de));
        let _ = fs::remove_file(local.join(MISE_A_JOUR));
        if posee && let Err(e) = poser(&p.fichier(&r.de), &p.exe) {
            journal::ecrire(&format!(
                "mise à jour : {} ne se repose pas ({e}) : {} est peut-être encore la version {} — à reposer à la main",
                r.de,
                p.exe.display(),
                r.vers
            ));
        }
        let passage = Passage {
            de: r.vers.clone(),
            vers: r.de.clone(),
            raison: raison.to_string(),
            session: self.session.clone(),
            heure: journal::date_et_heure(),
            ..Passage::default()
        };
        ecrire_passage(&local.join(RETOUR), &passage);
        match self.lancer() {
            Ok(mut ancienne) => match attendre_la_sante(&local, &r.de, &mut ancienne) {
                Issue::Bonne(t) => journal::ecrire(&format!(
                    "mise à jour : {} est revenue et répond (sa marque de santé en {:.1} s)",
                    r.de,
                    t.as_secs_f64()
                )),
                _ => journal::ecrire(&format!("mise à jour : {} relancée ne pose pas sa marque non plus", r.de)),
            },
            Err(e) => journal::ecrire(&format!("mise à jour : {} ne se relance pas ({e})", r.de)),
        }
        lever(&p.versions().join(VERROU));
        1
    }

    /// Lance `eve.exe` comme l'Eve qui s'est fermée : ses arguments, son environnement et son dossier courant (ceux du
    /// gardien, qui les tient d'elle).
    fn lancer(&self) -> io::Result<Child> {
        let mut c = Command::new(&self.programme);
        c.args(&self.eve);
        lancer_a_part(c)
    }
}

/// Comment s'est passée l'attente d'une marque de santé.
enum Issue {
    /// Posée, après ce temps.
    Bonne(Duration),
    /// L'Eve s'est arrêtée avant (son code de sortie).
    Arretee(Option<i32>),
    /// Rien en 90 s.
    Muette,
}

/// Attend la marque de santé de l'Eve `enfant`, de version `empreinte` ([`DELAI_SANTE`] au plus).
fn attendre_la_sante(local: &Path, empreinte: &str, enfant: &mut Child) -> Issue {
    let debut = Instant::now();
    let chemin = local.join(SANTE);
    loop {
        if fs::read_to_string(&chemin).is_ok_and(|t| sante_valable(&t, empreinte, enfant.id())) {
            return Issue::Bonne(debut.elapsed());
        }
        if let Ok(Some(statut)) = enfant.try_wait() {
            return Issue::Arretee(statut.code());
        }
        if debut.elapsed() >= DELAI_SANTE {
            return Issue::Muette;
        }
        thread::sleep(Duration::from_millis(250));
    }
}

/// `eve --surveiller-la-mise-a-jour …` : le gardien. Rend le code de sortie.
pub fn surveiller(args: &[String]) -> i32 {
    commencer();
    match Gardien::lire(args) {
        Ok(g) => g.mener(),
        Err(e) => {
            journal::ecrire(&format!("mise à jour : le gardien ne comprend pas ses arguments ({e})"));
            2
        }
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    /// Un dossier d'essai neuf, sous le dossier temporaire : jamais celui d'une vraie Eve.
    fn dossier(nom: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("eve-mise-a-jour-{nom}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn mots(m: &[&str]) -> Vec<String> {
        m.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn lire_ce_que_dit_un_programme() {
        assert_eq!(lire_version("Eve 0.1.0 (af9c445)\r\n"), Some(("0.1.0".into(), Some("af9c445".into()))));
        assert_eq!(lire_version("Eve 0.1.0"), Some(("0.1.0".into(), None)));
        assert_eq!(lire_version("Eve 0.1.0 (af9c445-modifie)").unwrap().1.as_deref(), Some("af9c445-modifie"));
        assert_eq!(lire_version("une ligne avant\nEve 0.2.0 (v1-3-gabc)").unwrap().1.as_deref(), Some("v1-3-gabc"));
        // Ce que dit ce programme-ci se relit tel quel.
        let (v, e) = lire_version(&ligne_de_version()).unwrap();
        assert_eq!((v.as_str(), e.as_deref()), (env!("CARGO_PKG_VERSION"), empreinte()));
        for rien in ["", "Python 3.12", "Eve ", "Eve 0.1.0 (af9c445"] {
            assert_eq!(lire_version(rien), None, "{rien:?}");
        }
    }

    /// La révision, comme la fenêtre la montre : sans les zéros de la fin.
    #[test]
    fn la_revision_se_dit_en_court() {
        for (version, revision) in [("7.0.0", "v7"), ("7.1.0", "v7.1"), ("7.1.2", "v7.1.2"), ("8.0.1", "v8.0.1")] {
            assert_eq!(revision_de(version), revision, "{version}");
        }
        assert_eq!(revision(), revision_de(env!("CARGO_PKG_VERSION")));
    }

    /// Seule une version commitée, sans changement, s'installe : elle dit où revenir.
    #[test]
    fn seule_une_version_commitee_s_installe() {
        assert_eq!(refus_d_empreinte(Some("af9c445")), None);
        assert_eq!(refus_d_empreinte(Some("v1.0-3-gaf9c445")), None);
        assert!(refus_d_empreinte(Some("af9c445-modifie")).is_some_and(|r| r.contains("commite d'abord")));
        assert!(refus_d_empreinte(None).is_some());
        for mauvaise in ["..\\eve", "a/b", "-x", "", "é"] {
            assert!(refus_d_empreinte(Some(mauvaise)).is_some(), "{mauvaise:?}");
        }
    }

    fn registre(empreintes: &[&str]) -> Registre {
        let mut r = Registre::default();
        for e in empreintes {
            r.poser(e, "hier");
        }
        r
    }

    fn empreintes(r: &Registre) -> Vec<&str> {
        r.posees.iter().map(|p| p.empreinte.as_str()).collect()
    }

    #[test]
    fn le_registre_garde_les_cinq_dernieres() {
        let r = registre(&["a", "b", "c", "d", "e", "f", "g"]);
        assert_eq!(empreintes(&r), ["c", "d", "e", "f", "g"]);
        assert_eq!((r.en_place(), r.precedente()), (Some("g"), Some("f")));
        assert_eq!(r.fichiers_gardes().len(), 5);
        assert!(r.fichiers_gardes().contains("c.exe") && !r.fichiers_gardes().contains("b.exe"));
        assert_eq!(registre(&["a"]).precedente(), None);
        assert_eq!(Registre::default().en_place(), None);
    }

    /// Revenir défait la dernière pose : le retour suivant remonte encore, jamais vers la version qu'on vient de
    /// quitter ; une mise à jour repart de là.
    #[test]
    fn revenir_remonte_sans_retourner_a_celle_qu_on_quitte() {
        let mut r = registre(&["a", "b", "c"]);
        r.apres(Sorte::Retour, Some("c"), "b", "maintenant");
        assert_eq!(empreintes(&r), ["a", "b"]);
        r.apres(Sorte::Retour, Some("b"), "a", "maintenant");
        assert_eq!(empreintes(&r), ["a"]);
        r.apres(Sorte::MiseAJour, Some("a"), "d", "maintenant");
        assert_eq!(empreintes(&r), ["a", "d"]);
        assert_eq!(r.posees[1].posee, "maintenant");
    }

    /// Une version posée à la main entre au registre avant la suivante ; une version d'avant les mises à jour, non.
    #[test]
    fn une_mise_a_jour_suit_la_version_en_place() {
        let mut r = registre(&["a"]);
        r.apres(Sorte::MiseAJour, Some("b"), "c", "maintenant");
        assert_eq!(empreintes(&r), ["a", "b", "c"]);
        r.apres(Sorte::MiseAJour, None, "d", "maintenant");
        assert_eq!(empreintes(&r), ["a", "b", "c", "d"]);
        let mut vide = Registre::default();
        vide.apres(Sorte::Retour, None, "a", "maintenant");
        assert_eq!(empreintes(&vide), ["a"]);
    }

    #[test]
    fn le_registre_s_ecrit_et_se_relit() {
        let d = dossier("registre");
        assert_eq!(Registre::lire(&d), Ok(Registre::default()), "absent : vide");
        let r = registre(&["a", "b"]);
        r.ecrire(&d).unwrap();
        assert_eq!(Registre::lire(&d), Ok(r));
        fs::write(d.join(REGISTRE), "{ pas du json").unwrap();
        assert!(Registre::lire(&d).is_err());
        assert_eq!(Registre::lire_pour_changer(&d), Registre::default(), "illisible : il repart de zéro");
        let _ = fs::remove_dir_all(&d);
    }

    /// La marque de santé ne vaut que pour la version attendue, lancée sous le numéro attendu.
    #[test]
    fn une_marque_de_sante_se_juge() {
        let marque = r#"{"empreinte": "af9c445", "pid": 4242, "heure": "2026-10-24 14:02:11"}"#;
        assert!(sante_valable(marque, "af9c445", 4242));
        assert!(!sante_valable(marque, "af9c445", 4243), "un autre lancement");
        assert!(!sante_valable(marque, "b000000", 4242), "une autre version");
        assert!(!sante_valable("{\"empreinte\": \"af9c445\"", "af9c445", 4242));
        assert!(sante_valable(r#"{"empreinte": "af9c445", "pid": 4242}"#, "af9c445", 4242));
    }

    #[test]
    fn le_gardien_relit_ses_arguments() {
        let r = Relance { vers: "abc1234".into(), de: "def5678".into(), sorte: Sorte::Retour };
        let programme = PathBuf::from(r"C:\Users\x\AppData\Local\Eve\programme\eve.exe");
        let eve = vec!["--native".to_string(), "--sans-fenetre".to_string()];
        let a = arguments_du_gardien(&r, 4242, Some("session-1"), &programme, &eve);
        assert_eq!(a[0], ARGUMENT_DU_GARDIEN);
        let g = Gardien::lire(&a[1..]).unwrap();
        let attendu =
            Gardien { pid: 4242, relance: r.clone(), session: Some("session-1".into()), programme, eve: eve.clone() };
        assert_eq!(g, attendu);
        let sans = arguments_du_gardien(&Relance { sorte: Sorte::MiseAJour, ..r }, 7, None, Path::new("eve.exe"), &[]);
        let g = Gardien::lire(&sans[1..]).unwrap();
        assert_eq!((g.relance.sorte, g.session, g.eve.len()), (Sorte::MiseAJour, None, 0));
        assert!(
            Gardien::lire(&mots(&["7", "..\\x", "b", "--programme", "eve.exe"])).is_err(),
            "une empreinte qui sort"
        );
        assert!(Gardien::lire(&mots(&["7", "a", "b"])).is_err(), "sans programme");
        assert!(Gardien::lire(&mots(&["7", "a", "b", "--programme", "eve.exe", "--force"])).is_err());
    }

    /// Le geste `mettre_a_jour` est une relance, telle quelle (le contrat entre les versions).
    #[test]
    fn le_geste_est_une_relance() {
        let r = Relance { vers: "abc1234".into(), de: "def5678".into(), sorte: Sorte::MiseAJour };
        let v = json!(r);
        assert_eq!(v, json!({ "vers": "abc1234", "de": "def5678", "sorte": "mise_a_jour" }));
        assert_eq!(serde_json::from_value::<Relance>(v).unwrap(), r);
        assert_eq!(json!(Sorte::Retour), json!("retour"));
    }

    /// Le programme d'une Eve qui tourne se lit dans son processus ; seul un `eve.exe` se remplace.
    #[test]
    fn le_programme_d_un_processus() {
        let moi = processus::image(std::process::id()).unwrap();
        assert_eq!(fs::canonicalize(&moi).unwrap(), fs::canonicalize(std::env::current_exe().unwrap()).unwrap());
        assert!(vivant(std::process::id()) && !vivant(0));
        assert!(nomme_eve(Path::new(r"C:\x\programme\EVE.exe")) && !nomme_eve(Path::new(r"C:\x\chrome.exe")));
    }

    /// Ce programme-ci porte la marque (il la cherche) ; un fichier quelconque, non.
    #[test]
    fn ce_programme_porte_la_marque() {
        assert!(porte_la_marque(&std::env::current_exe().unwrap()).unwrap());
        let d = dossier("marque");
        let autre = d.join("autre.exe");
        fs::write(&autre, b"MZ un programme d'avant, sans marque").unwrap();
        assert!(!porte_la_marque(&autre).unwrap());
        let r = Relance { vers: "abc1234".into(), de: "def5678".into(), sorte: Sorte::MiseAJour };
        let p = Programme { exe: d.join("eve.exe") };
        assert!(r.verifier(&p).is_err(), "les versions ne sont pas rangées");
        fs::create_dir_all(p.versions()).unwrap();
        fs::copy(std::env::current_exe().unwrap(), p.fichier("abc1234")).unwrap();
        fs::copy(&autre, p.fichier("def5678")).unwrap();
        assert!(r.verifier(&p).is_err(), "la version en place ne sait pas être le gardien");
        fs::copy(std::env::current_exe().unwrap(), p.fichier("def5678")).unwrap();
        assert_eq!(r.verifier(&p), Ok(()));
        let _ = fs::remove_dir_all(&d);
    }

    /// Une mise à jour à la fois : le verrou tient tant que son détenteur vit ; abandonné (son détenteur mort, ou trop
    /// vieux), il se reprend.
    #[test]
    fn une_mise_a_jour_a_la_fois() {
        let d = dossier("verrou");
        let v = Verrou::prendre(&d, "mise à jour vers abc1234").unwrap();
        assert!(matches!(Verrou::prendre(&d, "une autre"), Err(Empeche::Occupe(m)) if m.contains("abc1234")));
        drop(v);
        assert!(!d.join(VERROU).exists(), "lâché, il se lève");
        let mort = Tenue { pid: 0, depuis: secondes(), quoi: "un processus disparu".into() };
        fs::write(d.join(VERROU), serde_json::to_string(&mort).unwrap()).unwrap();
        let repris = Verrou::prendre(&d, "reprise").unwrap();
        // Confié à un autre processus (le gardien), il ne se lève plus avec ce programme-ci.
        repris.confier(std::process::id() + 1);
        assert!(d.join(VERROU).exists());
        let vieux = Tenue { pid: std::process::id(), depuis: secondes() - 16 * 60, quoi: "trop vieux".into() };
        fs::write(d.join(VERROU), serde_json::to_string(&vieux).unwrap()).unwrap();
        assert!(Verrou::prendre(&d, "après").is_ok(), "un verrou de plus de quinze minutes ne tient plus");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn poser_remplace_le_programme_d_un_geste() {
        let d = dossier("poser");
        let (source, programme) = (d.join("nouvelle.exe"), d.join("eve.exe"));
        fs::write(&source, b"la nouvelle").unwrap();
        poser(&source, &programme).unwrap();
        assert_eq!(fs::read(&programme).unwrap(), b"la nouvelle");
        fs::write(&programme, b"l'ancienne").unwrap();
        poser(&source, &programme).unwrap();
        assert_eq!(fs::read(&programme).unwrap(), b"la nouvelle");
        assert!(!d.join("eve.exe.tmp").exists());
        let registre = registre(&["garde"]);
        let p = Programme { exe: programme.clone() };
        fs::create_dir_all(p.versions()).unwrap();
        for f in ["garde.exe", "partie.exe", "copie.exe.tmp", "rangee-a-l-instant.exe"] {
            fs::write(p.versions().join(f), b"x").unwrap();
        }
        // « partie.exe » est tombée du registre il y a deux heures ; « rangee-a-l-instant.exe » attend peut-être sa
        // relance.
        let vieille = fs::File::options().write(true).open(p.versions().join("partie.exe")).unwrap();
        vieille.set_modified(SystemTime::now() - Duration::from_secs(2 * 3600)).unwrap();
        drop(vieille);
        nettoyer(&p, &registre);
        let restes: HashSet<String> =
            fs::read_dir(p.versions()).unwrap().flatten().map(|f| f.file_name().to_string_lossy().into()).collect();
        assert_eq!(restes, HashSet::from(["garde.exe".to_string(), "rangee-a-l-instant.exe".to_string()]));
        let _ = fs::remove_dir_all(&d);
    }

    /// Au démarrage, l'Eve lit ce que le gardien lui a laissé, une fois : le retour automatique se dit tout de suite ;
    /// une relance qui la vise se dit à sa marque de santé ; les deux reprennent la conversation.
    #[test]
    fn au_demarrage_l_eve_lit_son_passage() {
        let d = dossier("depart");
        let retour = Passage {
            de: "abc1234".into(),
            vers: "def5678".into(),
            raison: "elle n'a pas posé sa marque de santé en 90 s".into(),
            session: Some("s-1".into()),
            ..Passage::default()
        };
        ecrire_passage(&d.join(RETOUR), &retour);
        let depart = depart_dans(&d, "def5678");
        assert_eq!(depart.session.as_deref(), Some("s-1"));
        let (fil, voix) = depart.annonce.unwrap();
        assert!(fil.contains("abc1234") && fil.contains("90 s") && fil.contains("def5678"), "{fil}");
        assert_eq!(voix, "Ma nouvelle version n'a pas démarré : je suis revenue à la précédente.");
        assert!(!d.join(RETOUR).exists(), "il ne vaut qu'une fois");
        assert!(depart_dans(&d, "def5678").annonce.is_none());
        let relance = Passage {
            sorte: Some(Sorte::MiseAJour),
            de: "def5678".into(),
            vers: "abc1234".into(),
            session: Some("s-2".into()),
            ..Passage::default()
        };
        ecrire_passage(&d.join(MISE_A_JOUR), &relance);
        let depart = depart_dans(&d, "abc1234");
        assert_eq!(depart.session.as_deref(), Some("s-2"));
        assert!(depart.annonce.is_none());
        assert!(depart.a_la_sante.is_some_and(|(fil, _)| fil.contains("abc1234") && fil.contains("def5678")));
        ecrire_passage(&d.join(MISE_A_JOUR), &relance);
        let autre = depart_dans(&d, "0000000");
        assert!(autre.session.is_none() && autre.a_la_sante.is_none(), "une relance qui ne la vise pas");
        assert!(!d.join(MISE_A_JOUR).exists());
        let _ = fs::remove_dir_all(&d);
    }
}
