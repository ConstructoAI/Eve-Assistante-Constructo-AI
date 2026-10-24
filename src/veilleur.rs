//! Le veilleur : ce qu'Eve sait sans qu'on le lui demande — les lignes de la veille des courriels
//! (`%LOCALAPPDATA%\Adjointe\veille_courriels.log` : un courriel rangé, une pièce, une numérisation, un courriel
//! suspect) et les rendez-vous du jour (le calendrier en ligne d'Entrepreneur Général, lu avec le jeton d'Adjointe).
//! Il les rapporte au chef, qui décide de les dire (les alertes vocales) et s'en sert pour « quoi de neuf ».
//!
//! Le journal de la veille, c'est `classer_courriels.py` qui l'écrit (une ligne « TYPE AAAA-MM-JJ HH:MM : … » par
//! événement) ; le veilleur le suit comme `tail -F` et n'y écrit jamais. Ses phrases ne portent ni montant ni chemin,
//! comme les relevés de l'Adjointe. Le calendrier se lit par la route de la base (`/api/calendrier?donnees=1`) ; un
//! refus du serveur espace les essais à trente minutes : dix refus en quinze minutes bloqueraient l'adresse du bureau
//! pour tout le monde.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::chef::Evt;
use crate::{https, journal};

/// La sorte d'une nouvelle de la veille des courriels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sorte {
    /// Un courriel SUSPECT (règle 6) : ni lien, ni pièce, ni réponse.
    Suspect,
    /// Une pièce dont le nom dit « facture » (sortie d'un courriel, ou numérisée).
    Facture,
    /// Une autre pièce sortie d'un courriel, à classer.
    Piece,
    /// Un nouveau fichier dans `Scan\`.
    Scan,
    /// Un courriel rangé dans son dossier de projet.
    Range,
    /// Un courriel à confirmer (son projet n'est pas certain).
    AConfirmer,
    /// Un échec, une erreur ou une pause de la veille.
    Echec,
}

/// Une nouvelle de la veille des courriels.
#[derive(Clone, Debug)]
pub struct Nouvelle {
    pub sorte: Sorte,
    /// L'heure de la ligne, « 09:43 ».
    pub heure: String,
    /// Le projet, « 020 », s'il est nommé ; vide sinon (le texte le dit déjà : gardé pour qui voudra trier).
    #[allow(dead_code)]
    pub projet: String,
    /// Une phrase courte, pour l'oral et l'écran (« Un courriel suspect de Construction C. Lapointe »).
    pub texte: String,
    /// Arrivée depuis le démarrage d'Eve (à annoncer) ; `false` : une ligne plus ancienne du jour (pour « quoi de neuf »).
    pub nouvelle: bool,
}

/// Un rendez-vous du jour.
#[derive(Clone, Debug)]
pub struct RendezVous {
    /// Une clé stable (date, heure, titre) : un rendez-vous ne s'annonce qu'une fois.
    pub cle: String,
    /// « 2026-10-22 ».
    pub date: String,
    /// L'heure telle que le calendrier l'écrit (« 08 h 00 », « Appel à 08 h 30 ») ; vide pour un événement de la
    /// journée sans heure.
    pub heure: String,
    /// Les minutes qui restent avant le début (négatif : commencé) ; `None` : sans heure lisible.
    pub minutes: Option<i64>,
    pub titre: String,
    pub detail: String,
    /// La sorte de l'événement au calendrier (« confirme », « attente », « gros », « echeance », « ferie »).
    pub sorte: String,
}

/// Un jour du calendrier en ligne : sa date, son nom (« jeudi 22 octobre ») et ses événements (aucun : rien ce jour-là).
/// Le chef les joint à une question de calendrier : l'Adjointe répond sans relire la base, un aller-retour de moins.
#[derive(Clone, Debug)]
pub struct Jour {
    /// « 2026-10-22 » (le nom suffit à l'Adjointe : gardé pour qui voudra trier).
    #[allow(dead_code)]
    pub date: String,
    pub nom: String,
    pub evenements: Vec<RendezVous>,
}

/// Les jours que le veilleur garde du calendrier, aujourd'hui compris.
const JOURS_GARDES: i64 = 15;
/// Le journal de la veille se relit toutes les deux secondes.
const TOUR_COURRIELS: Duration = Duration::from_secs(2);
/// Le calendrier se relit toutes les deux minutes (une question de calendrier y lit sa réponse) ; les rappels se
/// vérifient toutes les vingt secondes.
const TOUR_CALENDRIER: Duration = Duration::from_secs(2 * 60);
const TOUR_RAPPELS: Duration = Duration::from_secs(20);
/// Après une panne de réseau, un nouvel essai dans une minute ; après un refus du serveur, dans trente.
const APRES_PANNE: Duration = Duration::from_secs(60);
const APRES_REFUS: Duration = Duration::from_secs(30 * 60);
const DELAI_HTTP: Duration = Duration::from_secs(20);
const ADRESSE_CALENDRIER: &str = "https://entrepreneur-general.ca/api/calendrier?donnees=1";
/// Au démarrage, au plus la fin du journal (il repart à neuf au-delà de 1 Mo) ; ensuite, au plus ceci par tour.
const PREMIERE_LECTURE_MAX: u64 = 4 * 1024 * 1024;
const LECTURE_MAX: u64 = 4 * 1024 * 1024;
/// Une ligne qui ne finit jamais s'oublie au-delà de cette taille.
const LIGNE_MAX: usize = 64 * 1024;
/// Les largeurs de `ligne(info)` dans classer_courriels.py : le correspondant (`MAX_CORRESPONDANT`) et l'objet.
const CORRESPONDANT_MAX: usize = 26;
const OBJET_MAX: usize = 58;
/// Un nom de fichier, dans une phrase.
const NOM_MAX: usize = 60;
/// Les genres de lignes qui donnent des nouvelles ; les autres (VEILLE, TRIEUR, RELEVÉ…) se taisent.
const GENRES_SUIVIS: [&str; 8] = ["SUSPECT", "PDF", "SCAN", "RANGÉ", "À CONFIRMER", "ECHEC", "ERREUR", "PAUSE"];

static COURRIELS_SUIVIS: AtomicBool = AtomicBool::new(false);
static CALENDRIER_SUIVI: AtomicBool = AtomicBool::new(false);

/// Démarre le veilleur, dans ses fils : la veille des courriels toujours ; le calendrier si le jeton est là.
/// `minutes_avant` : un rendez-vous s'annonce ce nombre de minutes avant son début.
pub fn demarrer(jeton: Option<String>, minutes_avant: u32, rapport: Sender<Evt>) {
    // Une seule fois chacun : deux fils sur le même journal annonceraient tout deux fois.
    if !COURRIELS_SUIVIS.swap(true, Ordering::SeqCst) {
        let r = rapport.clone();
        lancer("veilleur-courriels", move || suivre_la_veille(journal_de_la_veille(), r));
    }
    match jeton {
        Some(j) => {
            if !CALENDRIER_SUIVI.swap(true, Ordering::SeqCst) {
                lancer("veilleur-calendrier", move || suivre_le_calendrier(j, minutes_avant, rapport));
            }
        }
        None if !CALENDRIER_SUIVI.load(Ordering::SeqCst) => {
            journal::ecrire("Veilleur : pas de jeton d'Adjointe sur ce poste — les rendez-vous ne s'annoncent pas.");
        }
        None => {}
    }
}

fn lancer(nom: &str, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = thread::Builder::new().name(nom.into()).spawn(f) {
        journal::ecrire(&format!("Veilleur : le fil « {nom} » ne démarre pas : {e}"));
    }
}

/// Le journal de la veille, au même endroit que le calcule classer_courriels.py (`JOURNAL_VEILLE`).
fn journal_de_la_veille() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("Adjointe").join("veille_courriels.log")
}

// ------------------------------------------------------------------ la veille des courriels

fn suivre_la_veille(chemin: PathBuf, rapport: Sender<Evt>) {
    let mut suivi = Suivi::nouveau(chemin);
    loop {
        let (lignes, anciennes) = suivi.tour();
        if !lignes.is_empty() {
            let (aujourdhui, _) = maintenant();
            for texte in &lignes {
                let Some(l) = decouper(texte) else { continue };
                // Au démarrage, le journal tel qu'il était : seules ses lignes du jour servent (« quoi de neuf »).
                if anciennes && l.date != aujourdhui {
                    continue;
                }
                for n in ranger(&l, !anciennes) {
                    if rapport.send(Evt::Nouvelle(n)).is_err() {
                        return;
                    }
                }
            }
        }
        thread::sleep(TOUR_COURRIELS);
    }
}

/// Le journal de la veille, suivi comme `tail -F` : ce qui s'y ajoute, ligne par ligne. Il est ouvert le temps d'une
/// lecture seulement, pour ne jamais gêner la veille quand elle le met de côté (au-delà de 1 Mo, il devient « .1 »).
struct Suivi {
    chemin: PathBuf,
    /// L'octet où reprendre ; `None` tant que le fichier n'a pas été lu une première fois.
    position: Option<u64>,
    /// Le début d'une ligne pas encore terminée, gardé pour le tour suivant.
    reste: Vec<u8>,
}

impl Suivi {
    fn nouveau(chemin: PathBuf) -> Suivi {
        Suivi { chemin, position: None, reste: Vec::new() }
    }

    /// Les lignes terminées depuis le tour précédent, et si elles viennent de la première lecture (le journal tel qu'il
    /// était au démarrage : des lignes anciennes).
    fn tour(&mut self) -> (Vec<String>, bool) {
        let mut lignes = Vec::new();
        let taille = match fs::metadata(&self.chemin) {
            Ok(m) => m.len(),
            Err(e) => {
                // Absent au démarrage : tout ce qui y viendra sera nouveau. Absent plus tard (le temps d'une rotation)
                // ou illisible : on réessaie au tour suivant, sans bruit.
                if e.kind() == ErrorKind::NotFound && self.position.is_none() {
                    self.position = Some(0);
                }
                return (lignes, false);
            }
        };
        let premiere = self.position.is_none();
        if let Some(p) = self.position
            && taille < p
        {
            // Le fichier a rapetissé : la veille l'a mis de côté et en a commencé un autre. On repart du début.
            self.rattraper(p, &mut lignes);
            self.position = Some(0);
        }
        let mut position = self.position.unwrap_or(0);
        let mut entamee = false;
        if premiere && taille > PREMIERE_LECTURE_MAX {
            position = taille - PREMIERE_LECTURE_MAX;
            entamee = true;
        }
        if taille > position {
            let max = if premiere { taille - position } else { LECTURE_MAX };
            let Ok(octets) = lire_depuis(&self.chemin, position, max) else { return (lignes, false) };
            let mut neuf: &[u8] = &octets;
            if entamee {
                // La première ligne d'une lecture qui commence au milieu du fichier est coupée : elle s'oublie.
                neuf = neuf.iter().position(|&b| b == b'\n').map_or(&[][..], |i| &neuf[i + 1..]);
            }
            if position == 0 {
                neuf = neuf.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(neuf);
            }
            position += octets.len() as u64;
            self.reste.extend_from_slice(neuf);
            self.couper(&mut lignes, false);
        }
        self.position = Some(position);
        (lignes, premiere)
    }

    /// Après une rotation : la fin de l'ancien journal (« ….1 »), écrite juste avant et pas encore lue. Un « .1 » qui
    /// n'a pas bougé depuis cinq minutes n'est pas celui de cette rotation : il ne se relit pas.
    fn rattraper(&mut self, position: u64, lignes: &mut Vec<String>) {
        let mut ancien = self.chemin.clone().into_os_string();
        ancien.push(".1");
        let ancien = PathBuf::from(ancien);
        let recent = |m: &fs::Metadata| {
            m.len() >= position
                && m.modified().ok().and_then(|t| t.elapsed().ok()).is_some_and(|d| d < Duration::from_secs(300))
        };
        let fin =
            fs::metadata(&ancien).ok().filter(recent).and_then(|_| lire_depuis(&ancien, position, LECTURE_MAX).ok());
        match fin {
            Some(fin) => {
                self.reste.extend_from_slice(&fin);
                // L'ancien journal est clos : sa dernière ligne, même sans fin de ligne, est entière.
                self.couper(lignes, true);
            }
            None => self.reste.clear(),
        }
    }

    /// Les lignes terminées de `reste` passent dans `lignes` ; la fin sans retour de ligne attend, sauf si `tout`.
    fn couper(&mut self, lignes: &mut Vec<String>, tout: bool) {
        let mut debut = 0;
        while let Some(i) = self.reste[debut..].iter().position(|&b| b == b'\n') {
            pousser(lignes, &self.reste[debut..debut + i]);
            debut += i + 1;
        }
        self.reste.drain(..debut);
        if tout {
            let r = std::mem::take(&mut self.reste);
            pousser(lignes, &r);
        } else if self.reste.len() > LIGNE_MAX {
            self.reste.clear();
        }
    }
}

fn lire_depuis(chemin: &Path, position: u64, max: u64) -> std::io::Result<Vec<u8>> {
    let mut f = File::open(chemin)?;
    f.seek(SeekFrom::Start(position))?;
    let mut octets = Vec::new();
    f.take(max).read_to_end(&mut octets)?;
    Ok(octets)
}

/// Une ligne du journal, décodée sans exiger un UTF-8 parfait.
fn pousser(lignes: &mut Vec<String>, octets: &[u8]) {
    let octets = octets.strip_suffix(b"\r").unwrap_or(octets);
    let texte = String::from_utf8_lossy(octets);
    let texte = texte.trim();
    if !texte.is_empty() {
        lignes.push(texte.to_string());
    }
}

/// Une ligne du journal de la veille : « TYPE AAAA-MM-JJ HH:MM : corps ».
#[derive(Debug, PartialEq, Eq)]
struct Ligne<'a> {
    genre: &'a str,
    date: &'a str,
    heure: &'a str,
    corps: &'a str,
}

fn decouper(ligne: &str) -> Option<Ligne<'_>> {
    // Le genre : des majuscules et des espaces (« À CONFIRMER »), puis la date.
    let i = ligne.find(|c: char| c.is_ascii_digit())?;
    let genre = ligne[..i].strip_suffix(' ')?.trim();
    if genre.is_empty() || !genre.chars().all(|c| c.is_uppercase() || c == ' ') {
        return None;
    }
    let date = ligne.get(i..i + 10)?;
    let heure = ligne.get(i + 11..i + 16)?;
    let forme = |s: &str, motif: &str| {
        s.len() == motif.len()
            && s.bytes().zip(motif.bytes()).all(|(c, m)| if m == b'9' { c.is_ascii_digit() } else { c == m })
    };
    if !forme(date, "9999-99-99") || ligne.as_bytes().get(i + 10) != Some(&b' ') || !forme(heure, "99:99") {
        return None;
    }
    // « HH:MM : corps » ; le trieur glisse « (simulation) » avant les deux-points.
    let corps = ligne[i + 16..].split_once(" : ").map_or("", |(_, c)| c).trim();
    Some(Ligne { genre, date, heure, corps })
}

/// Ce que donne une ligne, avant d'en faire une `Nouvelle` : la sorte, le projet, la phrase.
type Brut = (Sorte, String, String);

/// Les nouvelles d'une ligne de la veille — aucune pour celles qui ne disent rien à la personne (VEILLE, TRIEUR,
/// RELEVÉ, À VÉRIFIER…). Une ligne PDF, RANGÉ ou SCAN peut en donner plusieurs : une par projet et par sorte.
fn ranger(l: &Ligne<'_>, nouvelle: bool) -> Vec<Nouvelle> {
    if !GENRES_SUIVIS.contains(&l.genre) {
        return Vec::new();
    }
    let bruts = match l.genre {
        "SUSPECT" => vec![suspect(l.corps)],
        "PDF" => pieces(l.corps),
        "SCAN" => numerisations(l.corps),
        "RANGÉ" => ranges(l.corps),
        "À CONFIRMER" => vec![a_confirmer(l.corps)],
        "ECHEC" | "ERREUR" | "PAUSE" => vec![(Sorte::Echec, String::new(), echec(l.genre, l.corps))],
        _ => Vec::new(),
    };
    bruts
        .into_iter()
        .map(|(sorte, projet, texte)| Nouvelle {
            sorte,
            heure: l.heure.to_string(),
            projet,
            texte: propre(&texte),
            nouvelle,
        })
        .filter(|n| !n.texte.is_empty())
        .collect()
}

/// « SUSPECT … : [marque] date  de  <expéditeur> <objet> — <raison> — <où> ; … »
fn suspect(corps: &str) -> Brut {
    let projet = corps
        .split_once("déjà au dossier du ")
        .and_then(|(_, r)| r.split([' ', ';', ',']).next())
        .filter(|p| est_projet(p))
        .unwrap_or_default();
    let texte = match courriel(corps) {
        Some(c) if connu(&c.correspondant) && c.recu => format!("Un courriel suspect {}", de(&c.correspondant)),
        Some(c) if connu(&c.correspondant) => format!("Un courriel suspect, envoyé à {}", c.correspondant),
        _ => "Un courriel suspect est arrivé".to_string(),
    };
    (Sorte::Suspect, projet.to_string(), texte)
}

/// « PDF … : N pièce(s) sortie(s) vers « 05. Communication\PDF à classer », à classer — 020. Nom : a [marque].pdf,
/// b [marque].pdf ; 002. Nom : c [marque].pdf » : une nouvelle par projet et par sorte (les factures à part).
fn pieces(corps: &str) -> Vec<Brut> {
    let Some((_, detail)) = corps.split_once(" — ") else { return Vec::new() };
    let mut sortie = Vec::new();
    for (dossier, liste) in segments(detail) {
        let projet = numero(dossier);
        let lieu = pour(&projet, dossier);
        let (factures, autres): (Vec<&str>, Vec<&str>) = fichiers(liste).into_iter().partition(|f| dit_facture(f));
        for (lot, sorte, un, plusieurs) in [
            (factures, Sorte::Facture, "Une facture est arrivée", "factures sont arrivées"),
            (autres, Sorte::Piece, "Une pièce est arrivée", "pièces sont arrivées"),
        ] {
            if lot.is_empty() {
                continue;
            }
            let noms: Vec<String> = lot.iter().map(|f| nom_de_piece(f)).filter(|n| !n.is_empty()).collect();
            sortie.push((sorte, projet.clone(), phrase(lot.len(), un, plusieurs, &lieu, &noms)));
        }
    }
    sortie
}

/// « SCAN … : nouveau fichier dans Scan\ — à classer (règle 7) : nom.pdf (66 Ko) », ou, au démarrage de la veille,
/// « SCAN … : N fichier(s) attendent dans Scan\ — à classer (règle 7) : a.pdf, b.pdf … ».
fn numerisations(corps: &str) -> Vec<Brut> {
    let apres = corps.rsplit_once(" : ").map_or("", |(_, a)| a);
    if corps.starts_with("nouveau fichier") {
        let fichier = sans_taille(apres);
        let nom = nettoyer_nom(sans_extension(fichier));
        let noms: Vec<String> = Some(nom).filter(|n| !n.is_empty()).into_iter().collect();
        let (sorte, un) = if dit_facture(fichier) {
            (Sorte::Facture, "Une facture numérisée est arrivée")
        } else {
            (Sorte::Scan, "Une numérisation est arrivée")
        };
        return vec![(sorte, String::new(), phrase(1, un, "", "", &noms))];
    }
    let total = nombre_en_tete(corps);
    if total == 0 {
        return Vec::new();
    }
    let liste = apres.trim().trim_end_matches('…').trim();
    let (factures, autres): (Vec<&str>, Vec<&str>) =
        liste.split(", ").map(str::trim).filter(|f| !f.is_empty()).partition(|f| dit_facture(f));
    let noms = |lot: &[&str]| -> Vec<String> {
        lot.iter().map(|f| nettoyer_nom(sans_extension(f))).filter(|n| !n.is_empty()).collect()
    };
    let mut sortie = Vec::new();
    if !factures.is_empty() {
        let t = phrase(
            factures.len(),
            "Une facture numérisée attend d'être classée",
            "factures numérisées attendent d'être classées",
            "",
            &noms(&factures),
        );
        sortie.push((Sorte::Facture, String::new(), t));
    }
    // Les fichiers que la ligne ne nomme pas (au-delà de quatre) comptent avec les numérisations.
    let reste = total.saturating_sub(factures.len()).max(autres.len());
    if reste > 0 {
        let t = phrase(
            reste,
            "Une numérisation attend d'être classée",
            "numérisations attendent d'être classées",
            "",
            &noms(&autres),
        );
        sortie.push((Sorte::Scan, String::new(), t));
    }
    sortie
}

/// « RANGÉ … : N courriel(s)[ dont K par le trieur] — 020. Nom : Yves X, Stephane ; 002. Nom : info » : une nouvelle
/// par projet. L'expéditeur n'y est que s'il se lit bien (ni adresse ni morceau d'adresse), deux au plus.
fn ranges(corps: &str) -> Vec<Brut> {
    let Some((tete, detail)) = corps.split_once(" — ") else { return Vec::new() };
    let total = nombre_en_tete(tete);
    let segs = segments(detail);
    let mut sortie = Vec::new();
    for &(dossier, liste) in &segs {
        // Un seul courriel : la liste est son correspondant, même s'il porte une virgule (« Tremblay, Jean »).
        let noms: Vec<String> = if segs.len() == 1 && total == 1 {
            vec![nom_entier(liste)]
        } else {
            liste.split(", ").map(nom_entier).filter(|n| !n.is_empty()).collect()
        };
        let n = if segs.len() == 1 { total.max(1) } else { noms.len().max(1) };
        let mut distincts: Vec<String> = Vec::new();
        for x in noms {
            if !distincts.contains(&x) {
                distincts.push(x);
            }
        }
        let qui = if !distincts.is_empty() && distincts.len() <= 2 && distincts.iter().all(|x| lisible(x)) {
            format!(" {}", de(&enumerer(&distincts, distincts.len())))
        } else {
            String::new()
        };
        let projet = numero(dossier);
        let au = au(&projet, dossier);
        let texte =
            if n == 1 { format!("Un courriel{qui} rangé{au}") } else { format!("{n} courriels{qui} rangés{au}") };
        sortie.push((Sorte::Range, projet, texte));
    }
    sortie
}

/// « À CONFIRMER … : [marque] date  de  <correspondant> <objet> → 016 : <motif> », ou « … : et N autre(s) — … ».
fn a_confirmer(corps: &str) -> Brut {
    if let Some(r) = corps.strip_prefix("et ") {
        let n = nombre_en_tete(r);
        let t = if n <= 1 {
            "Un autre courriel à confirmer".to_string()
        } else {
            format!("{n} autres courriels à confirmer")
        };
        return (Sorte::AConfirmer, String::new(), t);
    }
    let (objet, cles) = courriel(corps).map_or(("", ""), |c| couper_cles(c.reste));
    // Un seul projet possible : c'est lui qu'il faut confirmer ; plusieurs, aucun n'est nommé.
    let projet = if est_projet(cles) { cles.to_string() } else { String::new() };
    let objet = objet_abrege(objet);
    let texte = if objet.is_empty() {
        "Un courriel à confirmer".to_string()
    } else {
        format!("Un courriel à confirmer : {objet}")
    };
    (Sorte::AConfirmer, projet, texte)
}

/// ECHEC, ERREUR, PAUSE : ce qui ne va pas, sans le détail technique (il reste au journal de la veille).
fn echec(genre: &str, corps: &str) -> String {
    match genre {
        "PAUSE" if corps.contains("Outlook est fermé") => {
            "La veille des courriels est en pause : Outlook est fermé".into()
        }
        "PAUSE" => "La veille des courriels est en pause".into(),
        "ERREUR" if corps.contains("No space left") || corps.contains("Errno 28") => {
            "La veille des courriels est en erreur : le disque est plein".into()
        }
        "ERREUR" if corps.contains("Outlook ne répond pas") || corps.contains("RPC") => {
            "La veille des courriels est en erreur : Outlook ne répond pas".into()
        }
        "ERREUR" => "La veille des courriels est en erreur".into(),
        _ if corps.starts_with("trieur") => "Le trieur des courriels a échoué".into(),
        _ => {
            if let Some(r) = corps.strip_prefix("PDF « ") {
                let nom = nettoyer_nom(sans_extension(r.split_once(" » ").map_or(r, |(n, _)| n)));
                return if nom.is_empty() {
                    "Une pièce jointe n'a pas pu être sortie".into()
                } else {
                    format!("Une pièce jointe n'a pas pu être sortie : {nom}")
                };
            }
            match courriel(corps) {
                Some(c) if c.recu && lisible(&c.correspondant) => {
                    format!("Un courriel {} n'a pas pu être rangé", de(&c.correspondant))
                }
                Some(_) => "Un courriel n'a pas pu être rangé".into(),
                None => "La veille des courriels signale un échec".into(),
            }
        }
    }
}

/// Le début d'une ligne de courriel, tel que l'écrit `ligne(info)` dans classer_courriels.py :
/// « [marque] AAAA-MM-JJ HH:MM  de  <correspondant sur 26 colonnes> <objet> » (« à   » pour un envoi).
#[derive(Debug)]
struct Courriel<'a> {
    recu: bool,
    correspondant: String,
    /// Ce qui suit la colonne du correspondant : l'objet, puis la suite de la ligne.
    reste: &'a str,
}

fn courriel(corps: &str) -> Option<Courriel<'_>> {
    let r = corps.strip_prefix('[')?;
    let (marque, r) = r.split_once("] ")?;
    if marque.len() != 8 || !marque.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    // La date et l'heure du courriel (16 caractères), deux espaces, le sens sur trois colonnes, une espace.
    let r = r.get(16..)?.strip_prefix("  ")?;
    let (recu, r) = match (r.strip_prefix("de  "), r.strip_prefix("à   ")) {
        (Some(r), _) => (true, r),
        (None, Some(r)) => (false, r),
        _ => return None,
    };
    let fin = r.char_indices().nth(CORRESPONDANT_MAX).map_or(r.len(), |(i, _)| i);
    let reste = &r[fin..];
    Some(Courriel { recu, correspondant: nom_entier(&r[..fin]), reste: reste.strip_prefix(' ').unwrap_or(reste) })
}

/// « objet → 020 : motif » : l'objet et les projets possibles. L'objet peut porter une flèche ; les projets, non.
fn couper_cles(reste: &str) -> (&str, &str) {
    const FLECHE: &str = " → ";
    let mut depart = 0;
    while let Some(i) = reste[depart..].find(FLECHE) {
        let p = depart + i;
        let apres = &reste[p + FLECHE.len()..];
        let cles = apres.split_once(" : ").map_or(apres, |(c, _)| c).trim();
        if !cles.is_empty() && cles.chars().all(|c| c.is_ascii_alphanumeric() || ", ?-".contains(c)) {
            return (&reste[..p], cles);
        }
        depart = p + FLECHE.len();
    }
    (reste, "")
}

/// « 020. Nom : a, b ; 002. Nom : c » → [(« 020. Nom », « a, b »), (« 002. Nom », « c »)]. Les deux-points sont
/// interdits dans les noms de fichiers, de dossiers et de correspondants (`INTERDITS`) : ils séparent sûrement.
fn segments(detail: &str) -> Vec<(&str, &str)> {
    let morceaux: Vec<&str> = detail.split(" : ").collect();
    let mut sortie = Vec::new();
    let mut dossier = morceaux[0].trim();
    for (k, m) in morceaux.iter().enumerate().skip(1) {
        let (liste, suivant) =
            if k + 1 == morceaux.len() { (*m, "") } else { m.rsplit_once(" ; ").unwrap_or((*m, "")) };
        sortie.push((dossier, liste.trim()));
        dossier = suivant.trim();
    }
    sortie
}

/// Les fichiers d'une liste « a [marque].pdf, b [marque].pdf » : chacun finit par sa marque, ce qui coupe juste même
/// quand un nom porte une virgule.
fn fichiers(liste: &str) -> Vec<&str> {
    let mut sortie = Vec::new();
    let mut reste = liste.trim();
    while !reste.is_empty() {
        match fin_de_marque(reste) {
            Some(fin) => {
                sortie.push(reste[..fin].trim());
                reste = reste[fin..].trim_start_matches([',', ' ']);
            }
            None => {
                sortie.extend(reste.split(", ").map(str::trim).filter(|s| !s.is_empty()));
                break;
            }
        }
    }
    sortie
}

/// La fin (en octets) de la première marque « [0123abcd].pdf » du texte.
fn fin_de_marque(t: &str) -> Option<usize> {
    let b = t.as_bytes();
    let mut depart = 0;
    while let Some(j) = t[depart..].find('[') {
        let d = depart + j;
        if b.len() >= d + 14
            && b[d + 1..d + 9].iter().all(u8::is_ascii_hexdigit)
            && b[d + 9] == b']'
            && b[d + 10..d + 14].eq_ignore_ascii_case(b".pdf")
        {
            return Some(d + 14);
        }
        depart = d + 1;
    }
    None
}

/// « Facture 4512 [0f1e2d3c].pdf » → « Facture 4512 » : sans la marque de la veille ni l'extension.
fn nom_de_piece(fichier: &str) -> String {
    let f = fichier.trim();
    let base = match fin_de_marque(f) {
        Some(fin) if fin == f.len() => f[..fin - 14].trim_end(),
        _ => sans_extension(f),
    };
    nettoyer_nom(base)
}

fn sans_extension(nom: &str) -> &str {
    match nom.trim().rsplit_once('.') {
        Some((base, ext))
            if !base.trim().is_empty()
                && (1..=5).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            base.trim_end()
        }
        _ => nom.trim(),
    }
}

/// « nom.pdf (66 Ko) » → « nom.pdf ».
fn sans_taille(t: &str) -> &str {
    let t = t.trim();
    if let Some(debut) = t.strip_suffix(" Ko)").or_else(|| t.strip_suffix(" Mo)"))
        && let Some((nom, n)) = debut.rsplit_once(" (")
        && !n.is_empty()
        && n.chars().all(|c| c.is_ascii_digit() || c == ' ' || c == ',')
    {
        return nom.trim_end();
    }
    t
}

/// Un nom de fichier pour la phrase : les soulignés deviennent des espaces, sans montant, abrégé au besoin.
fn nettoyer_nom(nom: &str) -> String {
    let t = sans_montants(&nom.replace('_', " "));
    abreger(&t.split_whitespace().collect::<Vec<_>>().join(" "), NOM_MAX)
}

/// Le nom dit-il « facture » (« Facture », « FACTURÉ », « invoice ») ?
fn dit_facture(nom: &str) -> bool {
    let p = plier(nom);
    p.contains("factur") || p.contains("invoice")
}

/// En minuscules et sans accents, pour comparer.
fn plier(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| !('\u{300}'..='\u{36f}').contains(c))
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// Un nom tel que la veille l'écrit, plafonné à 26 caractères : à 26 tout juste, son dernier mot est sans doute coupé
/// (« Construction Exemple et Fi ») ; il s'oublie, et avec lui un « et » ou un « du » resté en bout.
fn nom_entier(nom: &str) -> String {
    let nom = nom.trim();
    if nom.chars().count() < CORRESPONDANT_MAX {
        return nom.to_string();
    }
    let Some((mut debut, _)) = nom.rsplit_once(char::is_whitespace) else { return nom.to_string() };
    loop {
        debut = debut.trim_end_matches(|c: char| c.is_whitespace() || ".-–—,;&+".contains(c));
        match debut.rsplit_once(char::is_whitespace) {
            Some((avant, mot)) if ["et", "de", "du", "des", "la", "le", "les", "and", "of"].contains(&mot) => {
                debut = avant
            }
            _ => break,
        }
    }
    if debut.is_empty() { nom.to_string() } else { debut.to_string() }
}

/// Un correspondant nommé (la veille écrit « inconnu » faute de mieux).
fn connu(nom: &str) -> bool {
    !nom.is_empty() && !nom.eq_ignore_ascii_case("inconnu")
}

/// Un nom qui se dit : ni adresse, ni morceau d'adresse (« ventes.exemple », « info »), ni « inconnu ».
fn lisible(nom: &str) -> bool {
    let n = nom.trim();
    let un_mot = !n.contains(char::is_whitespace);
    n.chars().count() >= 3
        && !n.contains('@')
        && n.chars().any(char::is_alphabetic)
        && connu(n)
        && !(un_mot && (n.contains(['.', '_']) || !n.chars().any(char::is_uppercase)))
}

/// « de Stephane », « d'Yves » : l'élision devant une voyelle.
fn de(nom: &str) -> String {
    match nom.chars().next() {
        Some(c) if "aeiouyàâäéèêëîïôöùûüAEIOUYÀÂÄÉÈÊËÎÏÔÖÙÛÜ".contains(c) => {
            format!("d'{nom}")
        }
        _ => format!("de {nom}"),
    }
}

/// « 020. Daniel Exemple » → « 020 ».
fn numero(dossier: &str) -> String {
    dossier.split_once(". ").map(|(n, _)| n).filter(|n| est_projet(n)).unwrap_or_default().to_string()
}

/// Un numéro de projet : « 020 », « 10A ».
fn est_projet(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 5
        && p.starts_with(|c: char| c.is_ascii_digit())
        && p.chars().all(|c| c.is_ascii_alphanumeric())
}

/// « pour le 020 », ou le nom du dossier quand il n'a pas de numéro.
fn pour(projet: &str, dossier: &str) -> String {
    if !projet.is_empty() {
        format!(" pour le {projet}")
    } else if !dossier.is_empty() {
        format!(" pour le dossier {dossier}")
    } else {
        String::new()
    }
}

fn au(projet: &str, dossier: &str) -> String {
    if !projet.is_empty() {
        format!(" au {projet}")
    } else if !dossier.is_empty() {
        format!(" au dossier {dossier}")
    } else {
        String::new()
    }
}

/// « Une facture est arrivée pour le 020 : A » ; « 3 factures sont arrivées pour le 020 : A, B et C ».
fn phrase(total: usize, un: &str, plusieurs: &str, lieu: &str, noms: &[String]) -> String {
    let mut t = if total <= 1 { format!("{un}{lieu}") } else { format!("{total} {plusieurs}{lieu}") };
    if !noms.is_empty() {
        t.push_str(" : ");
        t.push_str(&enumerer(noms, total));
    }
    t
}

/// « A », « A et B », « A, B et C » ; au-delà de trois, les deux premiers et le compte des autres.
fn enumerer(noms: &[String], total: usize) -> String {
    let total = total.max(noms.len());
    let montres = if total > 3 { &noms[..noms.len().min(2)] } else { noms };
    let mut morceaux: Vec<String> = montres.to_vec();
    match total - montres.len() {
        0 => {}
        1 => morceaux.push("une autre".into()),
        n => morceaux.push(format!("{n} autres")),
    }
    match morceaux.len() {
        0 => String::new(),
        1 => morceaux.remove(0),
        n => format!("{} et {}", morceaux[..n - 1].join(", "), morceaux[n - 1]),
    }
}

/// L'objet d'un courriel, pour l'oral : sans « RE : » ni « TR : », et, coupé par la veille à 58 caractères, sans son
/// dernier mot entamé.
fn objet_abrege(objet: &str) -> String {
    let mut o = objet.to_string();
    if objet.chars().count() >= OBJET_MAX {
        if !objet.ends_with(char::is_whitespace)
            && let Some(i) = o.rfind(char::is_whitespace)
        {
            o.truncate(i);
        }
        o = format!("{}…", o.trim_end_matches(|c: char| c.is_whitespace() || ",;:-–—".contains(c)));
    }
    let mut t = o.trim();
    while let Some((tete, suite)) = t.split_once(':')
        && ["re", "tr", "fw", "fwd", "ref", "réf", "aw"].contains(&tete.trim().to_lowercase().as_str())
    {
        t = suite.trim_start();
    }
    let t = sans_montants(t);
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn abreger(t: &str, max: usize) -> String {
    if t.chars().count() <= max {
        return t.to_string();
    }
    let coupe: String = t.chars().take(max).collect();
    let coupe = match coupe.rfind(' ') {
        Some(i) if i > coupe.len() / 2 => &coupe[..i],
        _ => coupe.as_str(),
    };
    format!("{}…", coupe.trim_end_matches(|c: char| c.is_whitespace() || ",;:-–—".contains(c)))
}

/// Le nombre en tête d'un texte (« 3 courriel(s) » → 3) ; 0 s'il n'y en a pas.
fn nombre_en_tete(t: &str) -> usize {
    let t = t.trim_start();
    let fin = t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    t[..fin].parse().unwrap_or(0)
}

/// La phrase finale : sans montant, les blancs réduits, sans parenthèses vides ni ponctuation orpheline au bout.
fn propre(texte: &str) -> String {
    let t = sans_montants(texte).replace("( )", "").replace("()", "");
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ").replace(" ,", ",");
    t.trim_matches(|c: char| c.is_whitespace() || ",;:-–—".contains(c)).to_string()
}

/// Le texte sans ses montants en dollars — « 4 335,37 $ », « 1234.56$ », « 25 k$ », « 12 dollars », « $1,234.56 » —,
/// comme `masquer_montants_heures` de journal.py : ni montant à l'oral, ni à l'écran.
fn sans_montants(texte: &str) -> String {
    let c: Vec<char> = texte.chars().collect();
    let mut sortie = String::with_capacity(texte.len());
    let mut i = 0;
    while i < c.len() {
        match montant(&c, i) {
            Some(fin) => i = fin,
            None => {
                sortie.push(c[i]);
                i += 1;
            }
        }
    }
    sortie
}

/// L'espace, l'insécable et la fine insécable de la typographie.
fn espace(c: char) -> bool {
    matches!(c, ' ' | '\u{a0}' | '\u{202f}')
}

fn de_mot(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// La fin d'un montant qui commence à `i`, s'il y en a un (les motifs `_MONTANT` et `_NOMBRE` de journal.py).
fn montant(c: &[char], i: usize) -> Option<usize> {
    let en = |k: usize| c.get(k).copied();
    let mille = |x: char| "kKmM".contains(x);
    if c[i] == '$' {
        // « $1,234.56 », « $ 25 k »
        let j = if en(i + 1).is_some_and(espace) { i + 2 } else { i + 1 };
        let fin = *nombres(c, j).first()?;
        let k = if en(fin).is_some_and(espace) { fin + 1 } else { fin };
        if en(k).is_some_and(mille) && !en(k + 1).is_some_and(de_mot) {
            return Some(k + 1);
        }
        return Some(fin);
    }
    if !c[i].is_ascii_digit() || (i > 0 && (de_mot(c[i - 1]) || matches!(c[i - 1], '.' | ','))) {
        return None;
    }
    for fin in nombres(c, i) {
        let mut j = if en(fin).is_some_and(espace) { fin + 1 } else { fin };
        if en(j).is_some_and(mille) {
            j += 1;
            if en(j).is_some_and(espace) {
                j += 1;
            }
        }
        if en(j) == Some('$') {
            // « 100 $ CAD », « 100 $CA »
            let k = if en(j + 1).is_some_and(espace) { j + 2 } else { j + 1 };
            if en(k) == Some('C') && en(k + 1) == Some('A') {
                let l = if en(k + 2) == Some('D') { k + 3 } else { k + 2 };
                if !en(l).is_some_and(de_mot) {
                    return Some(l);
                }
            }
            return Some(j + 1);
        }
        for mot in ["dollars", "dollar"] {
            let n = mot.chars().count();
            if commence(c, j, mot) && !en(j + n).is_some_and(de_mot) {
                return Some(j + n);
            }
        }
    }
    None
}

/// Les fins possibles d'un nombre qui commence à `i`, dans l'ordre des motifs de journal.py : « 4 335,37 »,
/// « 4,335.37 », « 4.335,37 », puis « 1234.56 », « 327,45 », « 40 ».
fn nombres(c: &[char], i: usize) -> Vec<usize> {
    let chiffres = |k: usize| c.get(k..).unwrap_or_default().iter().take_while(|x| x.is_ascii_digit()).count();
    let tete = chiffres(i);
    let mut fins = Vec::new();
    if tete == 0 {
        return fins;
    }
    // Le séparateur des milliers, puis les séparateurs décimaux permis après lui.
    type Forme = (fn(char) -> bool, &'static [char]);
    let formes: [Forme; 3] = [(espace, &['.', ',']), (|x| x == ',', &['.']), (|x| x == '.', &[','])];
    if tete <= 3 {
        for (separateur, decimales) in formes {
            let mut j = i + tete;
            let mut groupes = 0;
            while j < c.len() && separateur(c[j]) && chiffres(j + 1) == 3 {
                j += 4;
                groupes += 1;
            }
            if groupes == 0 {
                continue;
            }
            if j < c.len() && decimales.contains(&c[j]) && chiffres(j + 1) > 0 {
                j += 1 + chiffres(j + 1);
            }
            fins.push(j);
        }
    }
    let mut j = i + tete;
    if j < c.len() && matches!(c[j], '.' | ',') && chiffres(j + 1) > 0 {
        j += 1 + chiffres(j + 1);
    }
    fins.push(j);
    fins.dedup();
    fins
}

/// Le texte commence-t-il par `mot` à `i` (sans tenir compte de la casse) ?
fn commence(c: &[char], i: usize, mot: &str) -> bool {
    mot.chars().enumerate().all(|(k, m)| c.get(i + k).is_some_and(|x| x.to_lowercase().eq(m.to_lowercase())))
}

// ------------------------------------------------------------------ le calendrier

/// Un rendez-vous du jour et son début, en minutes depuis minuit (`None` : sans heure lisible).
#[derive(Clone, Debug)]
struct Prevu {
    rv: RendezVous,
    debut: Option<i64>,
}

impl Prevu {
    /// Le rendez-vous, avec les minutes qui restent à la minute du jour `minute`.
    fn a(&self, minute: i64) -> RendezVous {
        RendezVous { minutes: self.debut.map(|d| d - minute), ..self.rv.clone() }
    }
}

/// Pourquoi le calendrier ne s'est pas lu, et quand réessayer.
struct Panne {
    raison: String,
    attente: Duration,
}

fn suivre_le_calendrier(jeton: String, minutes_avant: u32, rapport: Sender<Evt>) {
    let mut jour = String::new();
    let mut prevus: Vec<Prevu> = Vec::new();
    let mut annonces: HashSet<String> = HashSet::new();
    let mut prochaine = Instant::now();
    // Une seule ligne au journal par série d'échecs ; la première lecture réussie se note aussi.
    let mut en_panne = false;
    let mut deja_lu = false;
    loop {
        let (date, _) = maintenant();
        if date != jour {
            // Un nouveau jour : ses rendez-vous, tout de suite ; ceux d'hier ne s'annoncent plus.
            jour = date;
            prevus.clear();
            annonces.clear();
            prochaine = Instant::now();
        }
        if Instant::now() >= prochaine {
            match lire_le_calendrier(&jeton, &jour) {
                Ok((lus, jours)) => {
                    if rapport.send(Evt::Calendrier { lu: journal::heure(), jours }).is_err() {
                        return;
                    }
                    if en_panne {
                        journal::ecrire("Calendrier en ligne : il se lit de nouveau.");
                        en_panne = false;
                    }
                    if !deja_lu {
                        let avec_heure = lus.iter().filter(|p| p.debut.is_some()).count();
                        journal::ecrire(&format!(
                            "Calendrier en ligne : {} rendez-vous aujourd'hui, dont {avec_heure} à heure fixe.",
                            lus.len()
                        ));
                        deja_lu = true;
                    }
                    prevus = lus;
                    let (_, minute) = maintenant();
                    if rapport.send(Evt::Agenda(prevus.iter().map(|p| p.a(minute)).collect())).is_err() {
                        return;
                    }
                    prochaine = Instant::now() + TOUR_CALENDRIER;
                }
                Err(p) => {
                    if !en_panne {
                        journal::ecrire(&format!(
                            "Calendrier en ligne : {} — nouvel essai dans {} min (une seule ligne jusqu'au retour).",
                            p.raison,
                            p.attente.as_secs().div_ceil(60)
                        ));
                        en_panne = true;
                    }
                    prochaine = Instant::now() + p.attente;
                }
            }
        }
        let (date, minute) = maintenant();
        for rv in a_annoncer(&prevus, &date, minute, minutes_avant, &mut annonces) {
            if rapport.send(Evt::RendezVous(rv)).is_err() {
                return;
            }
        }
        thread::sleep(TOUR_RAPPELS);
    }
}

/// Les quinze jours du calendrier, lus tout de suite (pour l'essai `--essai-claude`) : l'heure de la lecture et les
/// jours.
#[cfg_attr(not(test), allow(dead_code))]
pub fn lire_les_jours(jeton: &str) -> Result<(String, Vec<Jour>), String> {
    let (date, _) = maintenant();
    lire_le_calendrier(jeton, &date).map(|(_, jours)| (journal::heure(), jours)).map_err(|p| p.raison)
}

/// Une lecture du calendrier : les rendez-vous de `date`, et les quinze jours à partir d'elle. Les erreurs ne portent
/// jamais le jeton.
fn lire_le_calendrier(jeton: &str, date: &str) -> Result<(Vec<Prevu>, Vec<Jour>), Panne> {
    let panne = |raison: String, attente: Duration| Panne { raison, attente };
    let (code, corps) = https::get(ADRESSE_CALENDRIER, Some(jeton), DELAI_HTTP).map_err(|e| panne(e, APRES_PANNE))?;
    match code {
        200 => {
            let donnees = donnees_du_calendrier(&corps).map_err(|e| panne(e, TOUR_CALENDRIER))?;
            Ok((rendez_vous_dans(&donnees, date), jours_a_venir(&donnees, date)))
        }
        401 => Err(panne("le jeton d'Adjointe est refusé (401)".into(), APRES_REFUS)),
        429 => Err(panne("trop d'essais depuis ce poste (429)".into(), APRES_REFUS)),
        300..=399 => Err(panne(format!("le serveur renvoie vers sa page de connexion ({code})"), APRES_REFUS)),
        500..=599 => Err(panne(format!("le serveur répond {code}"), APRES_PANNE)),
        _ => Err(panne(format!("le calendrier répond {code}"), APRES_REFUS)),
    }
}

/// Les rendez-vous de `date` dans la réponse de `/api/calendrier?donnees=1` — `{"rev": n, "donnees": {"evenements":
/// {"AAAA-MM-JJ": [{t, h, ou, type, lieu, eq}, …]}}}` : ceux de la journée sans heure d'abord, puis par heure.
#[cfg_attr(not(test), allow(dead_code))]
fn rendez_vous_du_jour(corps: &[u8], date: &str) -> Result<Vec<Prevu>, String> {
    Ok(rendez_vous_dans(&donnees_du_calendrier(corps)?, date))
}

/// Les données de la réponse de `/api/calendrier?donnees=1`.
fn donnees_du_calendrier(corps: &[u8]) -> Result<Value, String> {
    let mut v: Value =
        serde_json::from_slice(corps).map_err(|_| "la réponse du calendrier n'est pas du JSON".to_string())?;
    match v.get_mut("donnees").map(Value::take) {
        Some(d) if d.is_object() => Ok(d),
        _ => Err("la réponse du calendrier n'a pas de données".into()),
    }
}

/// Les rendez-vous de `date` : ceux de la journée sans heure d'abord, puis par heure.
fn rendez_vous_dans(donnees: &Value, date: &str) -> Vec<Prevu> {
    let jour = donnees.get("evenements").and_then(|e| e.get(date)).and_then(Value::as_array);
    let mut prevus: Vec<Prevu> = jour.into_iter().flatten().filter_map(|e| prevu(e, date)).collect();
    prevus.sort_by_key(|p| p.debut.unwrap_or(-1));
    prevus
}

/// Les quinze jours du calendrier à partir de `aujourdhui`, les jours vides compris (« rien ce jour-là » est aussi une
/// réponse).
fn jours_a_venir(donnees: &Value, aujourdhui: &str) -> Vec<Jour> {
    let Some((a, m, j)) = lire_date(aujourdhui) else { return Vec::new() };
    let debut = jours_depuis_1970(a, m, j);
    (0..JOURS_GARDES)
        .map(|k| {
            let (a, m, j) = date_de(debut + k);
            let date = format!("{a:04}-{m:02}-{j:02}");
            let evenements = rendez_vous_dans(donnees, &date).into_iter().map(|p| p.rv).collect();
            Jour { nom: nom_du_jour(debut + k), date, evenements }
        })
        .collect()
}

const JOURS_DE_LA_SEMAINE: [&str; 7] = ["jeudi", "vendredi", "samedi", "dimanche", "lundi", "mardi", "mercredi"];
const MOIS: [&str; 12] = [
    "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre",
    "décembre",
];

/// « jeudi 22 octobre » (le 1er janvier 1970, jour zéro, était un jeudi).
fn nom_du_jour(jours: i64) -> String {
    let (_, m, j) = date_de(jours);
    let quantieme = if j == 1 { "1er".to_string() } else { j.to_string() };
    format!("{} {quantieme} {}", JOURS_DE_LA_SEMAINE[jours.rem_euclid(7) as usize], MOIS[(m - 1).clamp(0, 11) as usize])
}

/// « 2026-10-22 » : (2026, 10, 22).
fn lire_date(s: &str) -> Option<(i64, i64, i64)> {
    let mut p = s.splitn(3, '-').map(|x| x.parse::<i64>().ok());
    let (a, m, j) = (p.next()??, p.next()??, p.next()??);
    ((1..=12).contains(&m) && (1..=31).contains(&j)).then_some((a, m, j))
}

/// Les jours depuis le 1er janvier 1970 (le calendrier civil de Howard Hinnant).
fn jours_depuis_1970(a: i64, m: i64, j: i64) -> i64 {
    let a = if m <= 2 { a - 1 } else { a };
    let ere = a.div_euclid(400);
    let yoe = a - ere * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + j - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    ere * 146_097 + doe - 719_468
}

/// La date d'un nombre de jours depuis le 1er janvier 1970.
fn date_de(jours: i64) -> (i64, i64, i64) {
    let z = jours + 719_468;
    let ere = z.div_euclid(146_097);
    let doe = z - ere * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let j = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + ere * 400 + i64::from(m <= 2), m, j)
}

fn prevu(e: &Value, date: &str) -> Option<Prevu> {
    let champ = |nom: &str| match e.get(nom) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let (mut titre, mut detail) = (champ("t"), champ("ou"));
    if titre.is_empty() {
        titre = std::mem::take(&mut detail);
    }
    if titre.is_empty() {
        return None;
    }
    let heure = champ("h");
    let debut = premiere_heure(&heure);
    let rv = RendezVous {
        cle: format!("{date}|{heure}|{titre}"),
        date: date.into(),
        heure,
        minutes: None,
        titre,
        detail,
        sorte: champ("type"),
    };
    Some(Prevu { rv, debut })
}

/// Les rendez-vous de `date` qui commencent dans `minutes_avant` minutes ou moins, pas encore commencés : une fois
/// chacun. Ceux d'un autre jour (la lecture d'avant minuit) ne s'annoncent pas.
fn a_annoncer(
    prevus: &[Prevu],
    date: &str,
    minute: i64,
    minutes_avant: u32,
    annonces: &mut HashSet<String>,
) -> Vec<RendezVous> {
    let mut sortie = Vec::new();
    for p in prevus {
        let Some(debut) = p.debut else { continue };
        if p.rv.date != date {
            continue;
        }
        let reste = debut - minute;
        if reste > 0 && reste <= i64::from(minutes_avant) && annonces.insert(p.rv.cle.clone()) {
            sortie.push(p.a(minute));
        }
    }
    sortie
}

/// La première heure qui se lit dans le champ `h` du calendrier, en minutes depuis minuit : « 07 h 00 », « Appel à
/// 08 h 00 », « 07 h à 12 h » (7 h), « 13 h 30 », « 9 h », « 8:30 », « 8h30 », « midi ». `None` : aucune heure (ni
/// l'« après-midi », ni une durée comme « 327,45 h »).
fn premiere_heure(texte: &str) -> Option<i64> {
    let c: Vec<char> = texte.chars().collect();
    let mut i = 0;
    while i < c.len() {
        let debut_de_mot = i == 0 || !(c[i - 1].is_alphabetic() || c[i - 1] == '-');
        let fin_de_mot = |n: usize| !c.get(i + n).is_some_and(|x| x.is_alphabetic());
        if debut_de_mot && commence(&c, i, "midi") && fin_de_mot(4) {
            return Some(12 * 60);
        }
        if debut_de_mot && commence(&c, i, "minuit") && fin_de_mot(6) {
            return Some(0);
        }
        if c[i].is_ascii_digit() && (i == 0 || !c[i - 1].is_ascii_digit()) {
            let j = i + c[i..].iter().take_while(|x| x.is_ascii_digit()).count();
            // Ni une année, ni un nombre à virgule (une durée : « 7,5 h »).
            let decimale_avant = i >= 2 && matches!(c[i - 1], ',' | '.') && c[i - 2].is_ascii_digit();
            let decimale_apres =
                c.get(j).is_some_and(|x| matches!(x, ',' | '.')) && c.get(j + 1).is_some_and(char::is_ascii_digit);
            if j - i <= 2 && !decimale_avant && !decimale_apres {
                let heures = valeur(&c[i..j]);
                if heures <= 23
                    && let Some(minutes) = apres_l_heure(&c, j)
                {
                    return Some(heures * 60 + minutes);
                }
            }
            i = j;
            continue;
        }
        i += 1;
    }
    None
}

/// Ce qui suit les chiffres de l'heure — « h », « h30 », « h 30 », « heures », « :30 » — : les minutes, ou `None` si
/// ce n'est pas une heure (« 8 hommes »).
fn apres_l_heure(c: &[char], j: usize) -> Option<i64> {
    let deux_chiffres = |k: usize| -> Option<i64> {
        let ok = c.get(k).is_some_and(char::is_ascii_digit)
            && c.get(k + 1).is_some_and(char::is_ascii_digit)
            && !c.get(k + 2).is_some_and(char::is_ascii_digit);
        ok.then(|| valeur(&c[k..k + 2])).filter(|&m| m <= 59)
    };
    let mut k = j;
    while c.get(k).is_some_and(|&x| espace(x)) {
        k += 1;
    }
    match c.get(k) {
        Some(':') if k == j => deux_chiffres(k + 1),
        Some('h' | 'H') => {
            let mut m = k + 1;
            if c.get(m).is_some_and(|x| x.is_alphabetic()) {
                let suite = ["eures", "eure", "res", "rs", "r"].into_iter().find(|s| commence(c, m, s))?;
                m += suite.len();
                if c.get(m).is_some_and(|x| x.is_alphabetic()) {
                    return None;
                }
            }
            while c.get(m).is_some_and(|&x| espace(x)) {
                m += 1;
            }
            Some(deux_chiffres(m).unwrap_or(0))
        }
        _ => None,
    }
}

fn valeur(chiffres: &[char]) -> i64 {
    chiffres.iter().fold(0, |n, c| n * 10 + i64::from(c.to_digit(10).unwrap_or(0)))
}

/// La date locale (« 2026-10-22 ») et la minute du jour (0 à 1439).
#[cfg(windows)]
fn maintenant() -> (String, i64) {
    // SAFETY : GetLocalTime remplit une structure qu'il rend par valeur ; aucun pointeur ne survit à l'appel.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    (format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay), i64::from(t.wHour) * 60 + i64::from(t.wMinute))
}

/// Ailleurs que sous Windows : l'heure du journal.
#[cfg(not(windows))]
fn maintenant() -> (String, i64) {
    let h = journal::heure();
    let minute =
        h.split_once(':').map_or(0, |(a, b)| a.parse::<i64>().unwrap_or(0) * 60 + b.parse::<i64>().unwrap_or(0));
    (journal::date(), minute)
}

#[cfg(test)]
mod essais {
    use std::collections::BTreeMap;
    use std::io::Write;

    use super::*;

    /// Une ligne de courriel au format de `ligne(info)` (classer_courriels.py) : le correspondant sur 26 colonnes,
    /// l'objet coupé à 58.
    fn info(marque: &str, envoi: &str, recu: bool, correspondant: &str, objet: &str) -> String {
        let corr: String = correspondant.chars().take(CORRESPONDANT_MAX).collect();
        let obj: String = objet.chars().take(OBJET_MAX).collect();
        format!("[{marque}] {envoi}  {:<3} {corr:<26} {obj}", if recu { "de" } else { "à" })
    }

    fn nouvelles(ligne: &str) -> Vec<Nouvelle> {
        let l = decouper(ligne).unwrap_or_else(|| panic!("ligne illisible : {ligne}"));
        ranger(&l, true)
    }

    fn une(ligne: &str) -> Nouvelle {
        let mut n = nouvelles(ligne);
        assert_eq!(n.len(), 1, "{n:?}");
        n.remove(0)
    }

    #[test]
    fn une_ligne_se_decoupe() {
        let l = decouper("TRIEUR 2026-10-22 14:00 (simulation) : 2 courriel(s) jugé(s)").unwrap();
        assert_eq!((l.genre, l.date, l.heure, l.corps), ("TRIEUR", "2026-10-22", "14:00", "2 courriel(s) jugé(s)"));
        let l = decouper("À CONFIRMER 2026-10-22 08:05 : et 2 autre(s)").unwrap();
        assert_eq!((l.genre, l.heure), ("À CONFIRMER", "08:05"));
        for mauvaise in
            ["", "RANGÉ", "rangé 2026-10-22 09:00 : x", "RANGÉ 2026-1-22 09:00 : x", "RANGÉ2026-10-22 09:00 : x"]
        {
            assert!(decouper(mauvaise).is_none(), "{mauvaise}");
        }
    }

    #[test]
    fn un_suspect() {
        let corps =
            info("1a2b3c4d", "2026-10-22 09:12", true, "Construction Exemple et Fils Inc", "Facture 778 en retard");
        let n = une(&format!(
            "SUSPECT 2026-10-22 09:15 : {corps} — les réponses iraient à quelqu.un@exemple.com, et non à \
             factures@exemple.ca, l'adresse du carnet — ni rangé, ni PDF sorti ; `verifier_courriels.py voir` pour le détail"
        ));
        assert_eq!((n.sorte, n.heure.as_str(), n.projet.as_str()), (Sorte::Suspect, "09:15", ""));
        assert_eq!(n.texte, "Un courriel suspect de Construction Exemple");
        assert!(n.nouvelle);

        let corps = info("5e6f7a8b", "2026-10-22 09:20", true, "Isolation Exemple", "Changement de coordonnées");
        let n = une(&format!(
            "SUSPECT 2026-10-22 09:21 : {corps} — un changement de coordonnées bancaires — déjà au dossier du 031 ; \
             `verifier_courriels.py voir` pour le détail"
        ));
        assert_eq!((n.texte.as_str(), n.projet.as_str()), ("Un courriel suspect d'Isolation Exemple", "031"));

        let n = une("SUSPECT 2026-10-22 09:30 : format inconnu");
        assert_eq!(n.texte, "Un courriel suspect est arrivé");
    }

    #[test]
    fn des_pieces_et_des_factures() {
        let n = nouvelles(
            "PDF 2026-10-22 10:01 : 3 pièce(s) sortie(s) vers « 05. Communication\\PDF à classer », à classer — 031. Jean \
             Exemple : Facture_4512, version finale [0f1e2d3c].pdf, Plan R02 [0f1e2d3c].pdf ; 032. Marie Témoin : \
             INVOICE 778 (2) [aa11bb22].pdf",
        );
        let vu: Vec<(Sorte, &str, &str)> = n.iter().map(|n| (n.sorte, n.projet.as_str(), n.texte.as_str())).collect();
        assert_eq!(
            vu,
            [
                (Sorte::Facture, "031", "Une facture est arrivée pour le 031 : Facture 4512, version finale"),
                (Sorte::Piece, "031", "Une pièce est arrivée pour le 031 : Plan R02"),
                (Sorte::Facture, "032", "Une facture est arrivée pour le 032 : INVOICE 778 (2)"),
            ]
        );
        assert!(n.iter().all(|n| !n.texte.contains('\\') && !n.texte.contains(".pdf")));

        let n = une(
            "PDF 2026-10-22 10:11 : 4 pièce(s) sortie(s) vers « 05. Communication\\PDF à classer », à classer — 033. \
             Exemple : A [00000001].pdf, B [00000001].pdf, C [00000001].pdf, D [00000001].pdf",
        );
        assert_eq!(n.texte, "4 pièces sont arrivées pour le 033 : A, B et 2 autres");
    }

    #[test]
    fn des_numerisations() {
        let n = une(
            "SCAN 2026-10-22 11:38 : nouveau fichier dans Scan\\ — à classer (règle 7) : FACTURÉ Quincaillerie Exemple \
             1 234,56 $.pdf (66 Ko)",
        );
        assert_eq!(
            (n.sorte, n.texte.as_str()),
            (Sorte::Facture, "Une facture numérisée est arrivée : FACTURÉ Quincaillerie Exemple")
        );
        let n = une(
            "SCAN 2026-10-22 11:40 : nouveau fichier dans Scan\\ — à classer (règle 7) : Scan_2026-10-22.pdf (1585 Ko)",
        );
        assert_eq!((n.sorte, n.texte.as_str()), (Sorte::Scan, "Une numérisation est arrivée : Scan 2026-10-22"));

        let n = nouvelles(
            "SCAN 2026-10-22 07:05 : 6 fichier(s) attendent dans Scan\\ — à classer (règle 7) : a.pdf, facture b.pdf, \
             c.jpg, d.pdf …",
        );
        let vu: Vec<(Sorte, &str)> = n.iter().map(|n| (n.sorte, n.texte.as_str())).collect();
        assert_eq!(
            vu,
            [
                (Sorte::Facture, "Une facture numérisée attend d'être classée : facture b"),
                (Sorte::Scan, "5 numérisations attendent d'être classées : a, c et 3 autres"),
            ]
        );
    }

    #[test]
    fn des_courriels_ranges() {
        let n = nouvelles(
            "RANGÉ 2026-10-22 09:43 : 3 courriel(s) dont 1 par le trieur — 031. Jean Exemple : Yves Exemple, Yves Exemple \
             ; 032. Marie Témoin : ventes.exemple",
        );
        let vu: Vec<(Sorte, &str, &str)> = n.iter().map(|n| (n.sorte, n.projet.as_str(), n.texte.as_str())).collect();
        assert_eq!(
            vu,
            [
                (Sorte::Range, "031", "2 courriels d'Yves Exemple rangés au 031"),
                (Sorte::Range, "032", "Un courriel rangé au 032"),
            ]
        );
        let n = une("RANGÉ 2026-10-22 09:50 : 1 courriel(s) — 031. Jean Exemple : Tremblay, Jean");
        assert_eq!(n.texte, "Un courriel de Tremblay, Jean rangé au 031");
        let n =
            une("RANGÉ 2026-10-22 09:51 : 2 courriel(s) — 031. Jean Exemple : Ken Exemple, Fondation Exemple et Fils");
        assert_eq!(n.texte, "2 courriels de Ken Exemple et Fondation Exemple et Fils rangés au 031");
        let n = une("RANGÉ 2026-10-22 09:52 : 2 courriel(s) — 031. Jean Exemple : info, estimation@exemple.ca");
        assert_eq!(n.texte, "2 courriels rangés au 031");
        let n = une("RANGÉ 2026-10-22 09:53 : 1 courriel(s) — 031. Jean Exemple : Fondation Exemple Beaulieu et");
        assert_eq!(n.texte, "Un courriel de Fondation Exemple Beaulieu rangé au 031");
    }

    #[test]
    fn des_courriels_a_confirmer() {
        let corps = info(
            "9f8e7d6c",
            "2026-10-22 10:21",
            false,
            "estimation@exemple.ca",
            "RE: Soumission 4 335,37 $ → révision",
        );
        let n = une(&format!("À CONFIRMER 2026-10-22 10:28 : {corps} → 031 : nommé dans le corps seulement"));
        assert_eq!((n.sorte, n.projet.as_str()), (Sorte::AConfirmer, "031"));
        assert_eq!(n.texte, "Un courriel à confirmer : Soumission → révision");

        let corps = info(
            "0a0b0c0d",
            "2026-10-22 10:30",
            true,
            "Martin Exemple",
            "TR: Demande de prix pour la finition du sous-sol complet et du garage",
        );
        let n = une(&format!("À CONFIRMER 2026-10-22 10:31 : {corps} → 031, 032 : plusieurs chantiers nommés"));
        assert_eq!(n.projet, "");
        assert_eq!(n.texte, "Un courriel à confirmer : Demande de prix pour la finition du sous-sol complet…");

        let n = une("À CONFIRMER 2026-10-22 10:32 : et 3 autre(s) — `classer_courriels.py voir` pour la liste");
        assert_eq!(n.texte, "3 autres courriels à confirmer");
    }

    #[test]
    fn des_echecs_et_le_reste() {
        let cas = [
            (
                "PAUSE 2026-10-22 12:00 : Outlook est fermé — tour sauté ; la veille ne le relance jamais",
                "La veille des courriels est en pause : Outlook est fermé",
            ),
            (
                "ERREUR 2026-10-22 12:01 : OSError : [Errno 28] No space left on device — Outlook ne répond pas, ou attend une réponse à l'écran",
                "La veille des courriels est en erreur : le disque est plein",
            ),
            (
                "ERREUR 2026-10-22 12:02 : com_error : (-1, 'x') — Outlook ne répond pas, ou attend une réponse à l'écran",
                "La veille des courriels est en erreur : Outlook ne répond pas",
            ),
            ("ECHEC 2026-10-22 12:03 : trieur — le modèle refuse : surcharge", "Le trieur des courriels a échoué"),
        ];
        for (ligne, texte) in cas {
            let n = une(ligne);
            assert_eq!((n.sorte, n.texte.as_str()), (Sorte::Echec, texte));
        }
        let corps = info("12345678", "2026-10-22 12:00", true, "Plomberie Exemple", "Soumission");
        let n =
            une(&format!("ECHEC 2026-10-22 12:04 : PDF « Soumission_12.pdf » de {corps} → Outlook refuse la pièce"));
        assert_eq!(n.texte, "Une pièce jointe n'a pas pu être sortie : Soumission 12");
        let n = une(&format!("ECHEC 2026-10-22 12:05 : {corps} → déplacement refusé"));
        assert_eq!(n.texte, "Un courriel de Plomberie Exemple n'a pas pu être rangé");

        for muette in [
            "VEILLE 2026-10-22 08:00 : martin@exemple.ca, toutes les 10 min — 0 rangé(s)",
            "TRIEUR 2026-10-22 08:10 (simulation) : 1 courriel(s) jugé(s) — 0a1b2c3d → 031 (0.95)",
            "COMPTE RENDU 2026-10-22 08:20 : échec — retiré",
            "RELEVÉ 2026-10-22 08:30 : échec — sans message",
            "À VÉRIFIER 2026-10-22 08:40 : [0a1b2c3d] 2026-10-22 08:39  de  X → 031 — raison",
        ] {
            assert!(nouvelles(muette).is_empty(), "{muette}");
        }
    }

    #[test]
    fn ni_montant_ni_annee_perdue() {
        let cas = [
            ("Facture 4 335,37 $ Matériaux Exemple", "Facture Matériaux Exemple"),
            ("Dépôt de 30 % (1234.56$)", "Dépôt de 30 %"),
            ("$1,234.56 payé", "payé"),
            ("25 k$ de bois", "de bois"),
            ("12 dollars", ""),
            ("100 $ CAD reçu", "reçu"),
            ("Facture 2024 4 335,37 $", "Facture 2024"),
            ("Plan 2026 R02", "Plan 2026 R02"),
            ("Projet 020, lot 6.365.112", "Projet 020, lot 6.365.112"),
            ("25 km", "25 km"),
            ("A12 $", "A12 $"),
        ];
        for (avant, apres) in cas {
            assert_eq!(propre(avant), apres, "{avant}");
        }
    }

    #[test]
    fn les_noms() {
        assert_eq!(de("Yves"), "d'Yves");
        assert_eq!(de("Stephane"), "de Stephane");
        assert_eq!(de("Énergie Exemple"), "d'Énergie Exemple");
        for (nom, oui) in [
            ("Stephane", true),
            ("Ken Exemple Côté", true),
            ("9281-0000 Québec Inc", true),
            ("info", false),
            ("ventes.exemple", false),
            ("x@exemple.ca", false),
            ("inconnu", false),
            ("ab", false),
        ] {
            assert_eq!(lisible(nom), oui, "{nom}");
        }
        assert_eq!(nom_entier("Construction Exemple et Fi"), "Construction Exemple");
        assert_eq!(nom_entier("9999-0000 Québec Inc - Exe"), "9999-0000 Québec Inc");
        assert_eq!(nom_entier("estimation@exemplekl123.ca"), "estimation@exemplekl123.ca");
        assert_eq!(nom_entier("Court"), "Court");
    }

    #[test]
    fn les_heures_du_calendrier() {
        let cas = [
            ("07 h 00", Some(420)),
            ("Appel à 08 h 00", Some(480)),
            ("07 h à 12 h", Some(420)),
            ("13 h 30", Some(810)),
            ("9 h", Some(540)),
            ("8:30", Some(510)),
            ("8h30", Some(510)),
            ("16h45", Some(1005)),
            ("7h-15h30", Some(420)),
            ("8 heures", Some(480)),
            ("midi", Some(720)),
            ("Midi à 13 h", Some(720)),
            ("11 h à midi", Some(660)),
            ("Rendez-vous 2026-10-22 à 10 h", Some(600)),
            ("", None),
            ("En après-midi", None),
            ("avant-midi", None),
            ("8 hommes", None),
            ("327,45 h", None),
            ("7,5 h", None),
            ("24 h", None),
            ("Lot 6.365.112", None),
            ("2026-10-22", None),
            ("À confirmer", None),
        ];
        for (h, attendu) in cas {
            assert_eq!(premiere_heure(h), attendu, "« {h} »");
        }
    }

    const CALENDRIER: &str = r#"{"rev": 3, "donnees": {"evenements": {
        "2026-10-22": [
            {"t": "Livraison de gypse — chantier Exemple", "h": "07 h 00", "ou": "Chantier 031", "type": "confirme", "lieu": "x", "eq": "a"},
            {"t": "Appel au fournisseur", "h": "Appel à 08 h 30"},
            {"t": "Journée de formation", "type": "gros"},
            {"t": "", "ou": "Note sans titre", "h": "13 h 30"},
            {"t": "", "ou": ""}
        ],
        "2026-10-23": [{"t": "Demain", "h": "9 h"}]
    }, "lieux": {}, "equipes": {}}}"#;

    #[test]
    fn un_petit_calendrier() {
        let p = rendez_vous_du_jour(CALENDRIER.as_bytes(), "2026-10-22").unwrap();
        let vu: Vec<(&str, &str, Option<i64>)> =
            p.iter().map(|p| (p.rv.titre.as_str(), p.rv.heure.as_str(), p.debut)).collect();
        assert_eq!(
            vu,
            [
                ("Journée de formation", "", None),
                ("Livraison de gypse — chantier Exemple", "07 h 00", Some(420)),
                ("Appel au fournisseur", "Appel à 08 h 30", Some(510)),
                ("Note sans titre", "13 h 30", Some(810)),
            ]
        );
        assert_eq!(p[1].rv.cle, "2026-10-22|07 h 00|Livraison de gypse — chantier Exemple");
        assert_eq!((p[1].rv.detail.as_str(), p[3].rv.detail.as_str()), ("Chantier 031", ""));
        assert_eq!(p[1].a(6 * 60 + 50).minutes, Some(10));
        assert_eq!(p[0].a(600).minutes, None);
        assert!(rendez_vous_du_jour(CALENDRIER.as_bytes(), "2026-10-24").unwrap().is_empty());
        assert!(rendez_vous_du_jour(br#"{"rev": 1, "donnees": {}}"#, "2026-10-22").unwrap().is_empty());
        assert!(rendez_vous_du_jour(br#"{"rev": 1}"#, "2026-10-22").is_err());
        assert!(rendez_vous_du_jour(b"<html>connexion</html>", "2026-10-22").is_err());
    }

    #[test]
    fn un_rendez_vous_s_annonce_une_fois() {
        let jour = "2026-10-22";
        let p = rendez_vous_du_jour(CALENDRIER.as_bytes(), jour).unwrap();
        let mut annonces = HashSet::new();
        // 6 h 44 : la livraison de 7 h est à 16 minutes, pas encore.
        assert!(a_annoncer(&p, jour, 6 * 60 + 44, 15, &mut annonces).is_empty());
        // 6 h 50 : à 10 minutes.
        let a = a_annoncer(&p, jour, 6 * 60 + 50, 15, &mut annonces);
        assert_eq!(a.len(), 1);
        assert_eq!((a[0].titre.as_str(), a[0].minutes), ("Livraison de gypse — chantier Exemple", Some(10)));
        assert!(a_annoncer(&p, jour, 6 * 60 + 51, 15, &mut annonces).is_empty());
        // 8 h 15 : l'appel de 8 h 30, à 15 minutes ; 8 h 30 tout juste : commencé, plus d'annonce.
        assert_eq!(a_annoncer(&p, jour, 8 * 60 + 15, 15, &mut annonces).len(), 1);
        let mut neuves = HashSet::new();
        assert!(a_annoncer(&p, jour, 8 * 60 + 30, 15, &mut neuves).iter().all(|r| r.titre != "Appel au fournisseur"));
        assert!(a_annoncer(&p, jour, 6 * 60 + 50, 0, &mut HashSet::new()).is_empty());
        // Le lendemain, les rendez-vous de la veille se taisent.
        assert!(a_annoncer(&p, "2026-10-23", 6 * 60 + 50, 15, &mut HashSet::new()).is_empty());
    }

    /// Un dossier temporaire à soi, vidé à la fin.
    struct Dossier(PathBuf);

    impl Dossier {
        fn nouveau(nom: &str) -> Dossier {
            let d = std::env::temp_dir().join(format!("eve-veilleur-{nom}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).unwrap();
            Dossier(d)
        }
    }

    impl Drop for Dossier {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn ajouter(chemin: &Path, texte: &str) {
        let mut f = fs::OpenOptions::new().create(true).append(true).open(chemin).unwrap();
        f.write_all(texte.as_bytes()).unwrap();
    }

    #[test]
    fn le_journal_se_suit_ligne_par_ligne() {
        let d = Dossier::nouveau("suivi");
        let chemin = d.0.join("veille_courriels.log");
        ajouter(&chemin, "\u{feff}A ancienne\nB ancienne\r\n");
        let mut s = Suivi::nouveau(chemin.clone());
        assert_eq!(s.tour(), (vec!["A ancienne".to_string(), "B ancienne".to_string()], true));
        assert_eq!(s.tour(), (vec![], false));
        // Une ligne en cours d'écriture attend sa fin de ligne.
        ajouter(&chemin, "C en cours");
        assert_eq!(s.tour(), (vec![], false));
        ajouter(&chemin, " d'écriture\nD\n");
        assert_eq!(s.tour(), (vec!["C en cours d'écriture".to_string(), "D".to_string()], false));
        // Un octet qui n'est pas de l'UTF-8 ne perd pas la ligne.
        fs::OpenOptions::new().append(true).open(&chemin).unwrap().write_all(b"E \xff\n").unwrap();
        assert_eq!(s.tour(), (vec!["E \u{fffd}".to_string()], false));
    }

    #[test]
    fn la_rotation_du_journal() {
        let d = Dossier::nouveau("rotation");
        let chemin = d.0.join("veille_courriels.log");
        ajouter(&chemin, "AAAAAAAAAAAAAAAAAAAA\n");
        let mut s = Suivi::nouveau(chemin.clone());
        assert_eq!(s.tour().0, ["AAAAAAAAAAAAAAAAAAAA"]);
        // La veille écrit une dernière ligne, met le journal de côté (« .1 ») et en commence un autre.
        ajouter(&chemin, "BBBBBBBBBBBBBBBBBBBB\n");
        let mut ancien = chemin.clone().into_os_string();
        ancien.push(".1");
        fs::rename(&chemin, &ancien).unwrap();
        ajouter(&chemin, "C\n");
        assert_eq!(s.tour(), (vec!["BBBBBBBBBBBBBBBBBBBB".to_string(), "C".to_string()], false));
        ajouter(&chemin, "EEEEEEEEEE\n");
        assert_eq!(s.tour().0, ["EEEEEEEEEE"]);
        // Supprimé (le temps d'un tour, il manque : rien à dire), puis recréé plus petit, sans « .1 » à rattraper :
        // on repart du début.
        fs::remove_file(&ancien).unwrap();
        fs::remove_file(&chemin).unwrap();
        assert_eq!(s.tour(), (vec![], false));
        ajouter(&chemin, "D\n");
        assert_eq!(s.tour(), (vec!["D".to_string()], false));
    }

    #[test]
    fn un_journal_absent_au_demarrage() {
        let d = Dossier::nouveau("absent");
        let chemin = d.0.join("veille_courriels.log");
        let mut s = Suivi::nouveau(chemin.clone());
        assert_eq!(s.tour(), (vec![], false));
        // Né après le démarrage : tout ce qu'il porte est nouveau.
        let ligne = "RANGÉ 2026-10-22 09:00 : 1 courriel(s) — 031. Exemple : Stephane";
        ajouter(&chemin, &format!("{ligne}\n"));
        assert_eq!(s.tour(), (vec![ligne.to_string()], false));
    }

    /// Le vrai calendrier, avec le jeton d'Adjointe obtenu par le programme lui-même : rien que des comptes à l'écran.
    /// `cargo test --release -- --ignored --nocapture le_vrai_calendrier`
    #[test]
    #[ignore]
    fn le_vrai_calendrier() {
        let dossier = crate::reglages::charger().dossier_adjointe().expect("le dossier de l'Adjointe est introuvable");
        let jeton = crate::cerveau::jeton_mcp(&dossier).expect("aucun jeton d'Adjointe sur ce poste");
        let (code, corps) = https::get(ADRESSE_CALENDRIER, Some(&jeton), DELAI_HTTP).expect("la lecture a échoué");
        println!("HTTP {code} — {} octets", corps.len());
        assert_eq!(code, 200);
        let (date, _) = maintenant();
        let p = rendez_vous_du_jour(&corps, &date).expect("la réponse du calendrier est méconnaissable");
        println!(
            "aujourd'hui : {} événement(s), {} avec une heure lisible",
            p.len(),
            p.iter().filter(|p| p.debut.is_some()).count()
        );
        let v: Value = serde_json::from_slice(&corps).unwrap();
        let (mut total, mut avec_h, mut lisibles) = (0, 0, 0);
        for liste in v["donnees"]["evenements"].as_object().into_iter().flat_map(|o| o.values()) {
            for e in liste.as_array().into_iter().flatten() {
                total += 1;
                let h = e.get("h").and_then(Value::as_str).unwrap_or_default();
                if !h.trim().is_empty() {
                    avec_h += 1;
                    lisibles += usize::from(premiere_heure(h).is_some());
                }
            }
        }
        println!("tout le calendrier : {total} événement(s), {avec_h} avec un champ h, dont {lisibles} lisible(s)");
    }

    /// Le vrai journal de la veille : le nombre de lignes du jour par sorte (et, pour éprouver le découpage, de tout le
    /// journal), sans rien en afficher d'autre.
    /// `cargo test --release -- --ignored --nocapture le_vrai_journal_de_la_veille`
    #[test]
    #[ignore]
    fn le_vrai_journal_de_la_veille() {
        let mut s = Suivi::nouveau(journal_de_la_veille());
        let (lignes, premiere) = s.tour();
        assert!(premiere, "le journal de la veille est introuvable");
        let (date, _) = maintenant();
        let (mut du_jour, mut tout) = (BTreeMap::<String, usize>::new(), BTreeMap::<String, usize>::new());
        let (mut lignes_du_jour, mut muettes, mut douteuses) = (0, 0, 0);
        for t in &lignes {
            let Some(l) = decouper(t) else { continue };
            let n = ranger(&l, false);
            if n.is_empty() && GENRES_SUIVIS.contains(&l.genre) {
                muettes += 1;
            }
            lignes_du_jour += usize::from(l.date == date && !n.is_empty());
            for n in &n {
                *tout.entry(format!("{:?}", n.sorte)).or_default() += 1;
                if l.date == date {
                    *du_jour.entry(format!("{:?}", n.sorte)).or_default() += 1;
                }
                douteuses += usize::from(n.texte.contains(['\\', '$']) || n.texte.contains(" Ko)"));
            }
        }
        println!("{} ligne(s) lue(s), {lignes_du_jour} ligne(s) du jour qui donnent des nouvelles", lignes.len());
        println!("du jour, par sorte : {du_jour:?}");
        println!("tout le journal, par sorte : {tout:?}");
        println!("lignes suivies sans nouvelle : {muettes} ; phrases avec un chemin ou un montant : {douteuses}");
        assert_eq!((muettes, douteuses), (0, 0));
    }
}

#[cfg(test)]
mod essais_des_jours {
    use super::*;

    #[test]
    fn les_jours_se_comptent_et_se_nomment() {
        assert_eq!(jours_depuis_1970(1970, 1, 1), 0);
        assert_eq!(date_de(jours_depuis_1970(2026, 10, 22)), (2026, 10, 22));
        assert_eq!(nom_du_jour(jours_depuis_1970(2026, 10, 22)), "jeudi 22 octobre");
        assert_eq!(nom_du_jour(jours_depuis_1970(2026, 11, 1)), "dimanche 1er novembre");
        assert_eq!(date_de(jours_depuis_1970(2024, 2, 28) + 1), (2024, 2, 29));
        assert_eq!(date_de(jours_depuis_1970(2026, 12, 31) + 1), (2027, 1, 1));
    }

    #[test]
    fn quinze_jours_de_calendrier() {
        let donnees: Value = serde_json::from_str(
            r#"{"evenements": {"2026-10-23": [{"t": "Démolition", "h": "", "ou": "Chalet Exemple", "type": "confirme"}]}}"#,
        )
        .unwrap();
        let jours = jours_a_venir(&donnees, "2026-10-22");
        assert_eq!(jours.len(), 15);
        assert_eq!(jours[0].nom, "jeudi 22 octobre");
        assert!(jours[0].evenements.is_empty());
        assert_eq!(jours[1].evenements[0].titre, "Démolition");
        assert_eq!(jours[1].evenements[0].sorte, "confirme");
        assert_eq!(jours[14].date, "2026-11-05");
        assert!(jours_a_venir(&donnees, "pas une date").is_empty());
    }
}
