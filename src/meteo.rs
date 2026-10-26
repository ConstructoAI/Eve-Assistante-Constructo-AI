//! La météo du centre de commande (le lot 6.2b), celle du tableau HTML de la famille (« cdc:<ligne> » : une ligne de
//! `activite\centre-de-commande.html` ; l'inventaire : `docs\fenetre-native.md`, § 5) : un bulletin d'open-meteo toutes
//! les 15 min et sa ligne « VILLE · 5 °C · COUVERT » (cdc:565-573) ; la tempête possible, de novembre à avril
//! (cdc:671-678) ; et s'il neige, pour la neige des saisons (cdc:752).
//!
//! Le lieu vient de l'objet de la famille (`EVE_FAMILLE.meteo`, que la fenêtre a lu) : rien d'une famille n'est écrit
//! ici. La requête ne porte que les paramètres de la page et les coordonnées, au centième comme elle (un kilomètre
//! environ : assez pour le temps qu'il fait, trop peu pour trouver une maison) ; elle part sans jeton et sous un nom
//! neutre (`https::get_public`). Elle se fait dans un fil, jamais dans la fenêtre. Une panne se dit une fois au
//! journal. Le dernier bulletin reste bon une heure : une courte panne ne vide pas la ligne, une longue n'y laisse pas
//! la température d'il y a des heures (la page, elle, vide la ligne à la première panne, mais garde la tempête et la
//! neige jusqu'à la lecture suivante, si tard qu'elle vienne).

use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use crate::{https, journal};

/// Le service de la page : gratuit, sans clé ni compte.
const SERVICE: &str = "https://api.open-meteo.com/v1/forecast";
/// Le fuseau d'office, celui de la page : open-meteo y prend les bornes du jour (la neige et les rafales « du jour »).
const FUSEAU: &str = "America/Toronto";
/// La page relit la météo toutes les 15 min (cdc:573).
const PERIODE: Duration = Duration::from_secs(15 * 60);
/// Après une panne, un nouvel essai dans 1 min, puis 2, 4, 8, et 15 au plus : le réseau qui revient au réveil du
/// poste, ou après l'ouverture de la séance Windows (Eve part avec elle), se voit vite ; une longue panne n'envoie pas
/// plus de requêtes que la page.
const REPRISE: Duration = Duration::from_secs(60);
/// Un bulletin reste bon une heure après sa lecture, le temps de trois lectures manquées ; au-delà, plus de bulletin.
const VALIDITE: Duration = Duration::from_secs(60 * 60);
/// Le fil regarde l'horloge toutes les 30 s, plutôt que d'attendre 15 min d'un coup : une attente de Windows ne compte
/// pas le temps où le poste dort, et la météo se ferait attendre jusqu'à 15 min après son réveil.
const PAS: Duration = Duration::from_secs(30);
/// Le délai d'une lecture (celui du calendrier).
const DELAI: Duration = Duration::from_secs(20);
/// Au plus une requête par minute, toutes météos confondues : une fenêtre qui relancerait la météo à chaque image
/// n'écrirait pas pour autant trente fois par seconde à open-meteo.
const ENTRE_DEUX_REQUETES: Duration = Duration::from_secs(60);
/// Les codes où il neige : la neige des saisons tombe (cdc:752).
const NEIGE: [u32; 6] = [71, 73, 75, 77, 85, 86];

/// Un bulletin imposé : la vitrine de `eve --essai-theme` (un temps inventé, de saison, qui suit sa journée). Tant
/// qu'il l'est, toute météo le rend tel quel, sans fil ni requête. Eve ne l'impose jamais.
static IMPOSE: Mutex<Option<Bulletin>> = Mutex::new(None);

/// Impose ce bulletin à toutes les météos, celles qui tournent comme celles qui viendront (la vitrine de l'essai du
/// thème).
pub fn imposer(b: Bulletin) {
    *IMPOSE.lock().unwrap_or_else(PoisonError::into_inner) = Some(b);
}

fn impose() -> Option<Bulletin> {
    IMPOSE.lock().unwrap_or_else(PoisonError::into_inner).clone()
}
/// La tempête possible (cdc:673-674) : 5 cm de neige, ou des rafales de 70 km/h, prévus pour le jour.
const NEIGE_DE_TEMPETE: f64 = 5.0;
const RAFALES_DE_TEMPETE: f64 = 70.0;

/// Où lire la météo.
#[derive(Clone, Debug, PartialEq)]
pub struct Lieu {
    /// Le nom de la ville, que la ligne écrit en capitales ; vide, la ligne commence à la température.
    pub ville: String,
    pub latitude: f64,
    pub longitude: f64,
    /// Le fuseau (un nom IANA, « America/Toronto »), dont le jour d'open-meteo prend ses bornes.
    pub fuseau: String,
}

/// Le lieu de la météo dans l'objet de la famille (`EVE_FAMILLE`, ou `Null`) :
/// `{ meteo: { ville: "Montréal", latitude: 45.50, longitude: -73.57, fuseau: "America/Toronto" } }`. La latitude et
/// la longitude sont des nombres, ou des chaînes qui en sont (« 45,50 » aussi) ; sans fuseau, America/Toronto, celui
/// de la page ; la ville peut manquer. Sans coordonnées lisibles, pas de lieu, et pas de météo.
pub fn lieu_de(famille: &Value) -> Option<Lieu> {
    let m = famille.get("meteo")?;
    let nombre = |cle: &str| {
        let x = match m.get(cle)? {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().replace(',', ".").parse::<f64>().ok(),
            _ => None,
        };
        x.filter(|x| x.is_finite())
    };
    let latitude = nombre("latitude").filter(|x| x.abs() <= 90.0)?;
    let longitude = nombre("longitude").filter(|x| x.abs() <= 180.0)?;
    let texte = |cle: &str| m.get(cle).and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();
    let fuseau = Some(texte("fuseau")).filter(|f| !f.is_empty()).unwrap_or_else(|| FUSEAU.to_string());
    Some(Lieu { ville: texte("ville"), latitude, longitude, fuseau })
}

/// Un bulletin : ce que la page tire d'une réponse d'open-meteo.
#[derive(Clone, Debug, PartialEq)]
pub struct Bulletin {
    /// La ligne de la page, « MONTRÉAL · 5 °C · COUVERT » : en haut de la fenêtre, et sur l'écran de veille.
    pub ligne: String,
    /// Le code WMO du temps qu'il fait.
    pub code: u32,
    /// La température, en °C, telle que lue (la ligne l'arrondit).
    pub temperature: f64,
    /// Il neige (71, 73, 75, 77, 85, 86) : la neige des saisons tombe (cdc:752).
    pub il_neige: bool,
    /// La neige prévue pour le jour, en cm, et ses plus fortes rafales, en km/h ; 0 quand la réponse ne les a pas,
    /// comme dans la page.
    pub neige_cm: f64,
    pub rafales_kmh: f64,
    /// La tempête possible, selon le mois du poste au moment de la lecture.
    pub tempete: Option<Tempete>,
    /// Les jours de la prévision (aujourd'hui, demain, après-demain), pour la carte de la météo du tableau de bord.
    pub jours: Vec<Jour>,
}

/// Un jour de la prévision : sa date (« 2026-10-27 »), ses températures la plus haute et la plus basse (°C, telles
/// que lues), le code WMO du temps qu'il fera, la plus forte chance de précipitations (en %) ; ce que la réponse n'a
/// pas manque.
#[derive(Clone, Debug, PartialEq)]
pub struct Jour {
    pub date: String,
    pub max: Option<f64>,
    pub min: Option<f64>,
    pub code: Option<u32>,
    pub precipitations: Option<f64>,
}

/// Une tempête possible (cdc:671-678) : la neige et les rafales prévues pour le jour, telles que lues — le panneau les
/// arrondit (« 12 cm de neige · rafales 80 km/h »).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tempete {
    pub neige_cm: f64,
    pub rafales_kmh: f64,
}

/// La tempête de la page (cdc:673-674) : de novembre à avril (`mois`, de 1 à 12), 5 cm de neige ou des rafales de
/// 70 km/h prévus pour le jour.
pub fn tempete_de(mois: u32, neige_cm: f64, rafales_kmh: f64) -> Option<Tempete> {
    let saison = matches!(mois, 11 | 12 | 1..=4);
    let forte = neige_cm >= NEIGE_DE_TEMPETE || rafales_kmh >= RAFALES_DE_TEMPETE;
    (saison && forte).then_some(Tempete { neige_cm, rafales_kmh })
}

/// La requête de la page (cdc:570), pour ce lieu : ses paramètres, les coordonnées au centième comme elle, le fuseau
/// encodé (« America%2FToronto ») ; et, pour la carte de la météo du tableau de bord, trois jours de prévision (leurs
/// températures, leur temps, leur chance de précipitations). Rien d'autre : ni la ville, ni un nom.
pub fn requete(lieu: &Lieu) -> String {
    format!(
        "{SERVICE}?latitude={:.2}&longitude={:.2}&current=temperature_2m,weather_code\
         &daily=snowfall_sum,wind_gusts_10m_max,temperature_2m_max,temperature_2m_min,weather_code,\
         precipitation_probability_max&forecast_days=3&timezone={}",
        lieu.latitude,
        lieu.longitude,
        encoder(&lieu.fuseau)
    )
}

/// Le bulletin d'une réponse d'open-meteo, lu comme la page : la température et le code de `current` ; le premier jour
/// de `daily.snowfall_sum` et de `daily.wind_gusts_10m_max`, 0 s'il manque (`(dj.snowfall_sum || [0])[0] || 0`).
/// `mois`, de 1 à 12 : le mois du poste, que la page prend pour la saison des tempêtes. À part pour les essais.
pub fn lire(json: &[u8], lieu: &Lieu, mois: u32) -> Result<Bulletin, String> {
    let v: Value = serde_json::from_slice(json).map_err(|_| "la réponse d'open-meteo n'est pas du JSON".to_string())?;
    if v.get("error").and_then(Value::as_bool) == Some(true) {
        let pourquoi = raison(&v).unwrap_or_else(|| "sans raison dite".into());
        return Err(format!("open-meteo refuse la demande : {pourquoi}"));
    }
    let actuel = v.get("current");
    let temperature = actuel
        .and_then(|a| a.get("temperature_2m"))
        .and_then(Value::as_f64)
        .ok_or("la réponse d'open-meteo n'a pas de température")?;
    let code = actuel
        .and_then(|a| a.get("weather_code"))
        .and_then(entier)
        .ok_or("la réponse d'open-meteo n'a pas le code du temps")?;
    let du_jour = |cle: &str| {
        v.get("daily").and_then(|d| d.get(cle)).and_then(|a| a.get(0)).and_then(Value::as_f64).unwrap_or(0.0)
    };
    let (neige_cm, rafales_kmh) = (du_jour("snowfall_sum"), du_jour("wind_gusts_10m_max"));
    Ok(Bulletin {
        ligne: ligne(&lieu.ville, temperature, code),
        code,
        temperature,
        il_neige: NEIGE.contains(&code),
        neige_cm,
        rafales_kmh,
        tempete: tempete_de(mois, neige_cm, rafales_kmh),
        jours: jours_de(v.get("daily")),
    })
}

/// Les jours de la prévision d'une réponse (`daily` : une colonne par grandeur, une rangée par jour de `time`) ; une
/// valeur nulle ou absente manque.
fn jours_de(quotidien: Option<&Value>) -> Vec<Jour> {
    let colonne = |cle: &str| quotidien.and_then(|d| d.get(cle)).and_then(Value::as_array);
    let Some(dates) = colonne("time") else { return Vec::new() };
    dates
        .iter()
        .enumerate()
        .filter_map(|(i, date)| {
            let valeur = |cle: &str| colonne(cle).and_then(|a| a.get(i)).and_then(Value::as_f64);
            Some(Jour {
                date: date.as_str()?.to_string(),
                max: valeur("temperature_2m_max"),
                min: valeur("temperature_2m_min"),
                code: colonne("weather_code").and_then(|a| a.get(i)).and_then(entier),
                precipitations: valeur("precipitation_probability_max"),
            })
        })
        .collect()
}

/// La météo d'un lieu, que son fil tient à jour : une lecture tout de suite, puis toutes les 15 min. Lâchée, son fil
/// s'arrête aussitôt (à la fin d'une lecture en cours, s'il y en a une).
///
/// Dans la fenêtre : la relancer seulement quand le lieu change, et la lâcher quand il n'y en a plus —
/// `if lieu.as_ref() != meteo.as_ref().map(Meteo::lieu) { meteo = lieu.map(Meteo::lancer) }`.
pub struct Meteo {
    lieu: Lieu,
    partage: Arc<Mutex<Partage>>,
    /// Lâché avec la météo : son fil le voit et s'arrête.
    _arret: mpsc::Sender<()>,
}

/// Ce que le fil laisse à la fenêtre.
#[derive(Default)]
struct Partage {
    /// Le dernier bulletin, et l'heure de sa lecture.
    dernier: Option<(Bulletin, SystemTime)>,
    /// La raison de la panne en cours, depuis la dernière lecture réussie.
    panne: Option<String>,
}

impl Meteo {
    /// Lance la lecture de ce lieu, dans son fil (aucun sous un bulletin imposé : [`imposer`]).
    pub fn lancer(lieu: Lieu) -> Meteo {
        let partage: Arc<Mutex<Partage>> = Arc::default();
        let (arret, attente) = mpsc::channel();
        let (l, p) = (lieu.clone(), Arc::clone(&partage));
        if impose().is_none()
            && let Err(e) = thread::Builder::new().name("meteo".into()).spawn(move || suivre(&l, &p, &attente))
        {
            journal::ecrire(&format!("Météo : le fil ne démarre pas : {e}"));
        }
        Meteo { lieu, partage, _arret: arret }
    }

    /// Le lieu de cette météo.
    pub fn lieu(&self) -> &Lieu {
        &self.lieu
    }

    /// Le dernier bulletin, s'il a moins d'une heure ; rien avant la première lecture, ni après une heure de panne. Un
    /// bulletin imposé passe avant tout.
    pub fn bulletin(&self) -> Option<Bulletin> {
        if let Some(b) = impose() {
            return Some(b);
        }
        let p = self.partage.lock().unwrap_or_else(PoisonError::into_inner);
        let (b, lu) = p.dernier.as_ref()?;
        frais(*lu, SystemTime::now()).then(|| b.clone())
    }

    /// La raison de la panne en cours, depuis la dernière lecture réussie (pour l'essai : la page n'en dit rien).
    pub fn panne(&self) -> Option<String> {
        self.partage.lock().unwrap_or_else(PoisonError::into_inner).panne.clone()
    }
}

/// Un bulletin lu à `lu` est-il encore bon à `maintenant` ? Une heure ; une horloge qui a reculé le garde jusqu'à la
/// lecture suivante.
fn frais(lu: SystemTime, maintenant: SystemTime) -> bool {
    !matches!(maintenant.duration_since(lu), Ok(age) if age > VALIDITE)
}

/// Le fil de la météo : une lecture tout de suite, puis quand `Suivi` le dit ; il s'arrête quand la météo est lâchée
/// (l'émetteur d'`arret` avec elle).
fn suivre(lieu: &Lieu, partage: &Mutex<Partage>, arret: &mpsc::Receiver<()>) {
    let mut suivi = Suivi::nouveau(SystemTime::now());
    loop {
        let maintenant = SystemTime::now();
        if suivi.a_lire(maintenant) {
            match reserver() {
                Err(reste) => suivi.prochaine = maintenant + reste,
                Ok(()) => {
                    let lu = lire_maintenant(lieu);
                    let fin = SystemTime::now();
                    for ligne in suivi.apres(&lu, fin) {
                        journal::ecrire(&ligne);
                    }
                    let mut p = partage.lock().unwrap_or_else(PoisonError::into_inner);
                    match lu {
                        Ok(b) => *p = Partage { dernier: Some((b, fin)), panne: None },
                        Err(e) => p.panne = Some(e),
                    }
                }
            }
        }
        if !matches!(arret.recv_timeout(PAS), Err(mpsc::RecvTimeoutError::Timeout)) {
            return;
        }
    }
}

/// Le fil d'une météo entre deux lectures : quand lire la suivante, et ce qui se dit au journal.
struct Suivi {
    prochaine: SystemTime,
    /// L'attente après une panne, qui double à chaque échec, jusqu'à la période.
    reprise: Duration,
    en_panne: bool,
    deja_lu: bool,
    /// La tempête du dernier bulletin s'est dite au journal.
    tempete_dite: bool,
}

impl Suivi {
    fn nouveau(maintenant: SystemTime) -> Suivi {
        Suivi { prochaine: maintenant, reprise: REPRISE, en_panne: false, deja_lu: false, tempete_dite: false }
    }

    /// L'heure de lire est-elle venue ? Aussi quand l'horloge du poste a reculé de plus d'une période (une heure
    /// changée à la main) : la lecture suivante ne s'éloigne pas d'autant.
    fn a_lire(&self, maintenant: SystemTime) -> bool {
        !matches!(self.prochaine.duration_since(maintenant), Ok(reste) if !reste.is_zero() && reste <= PERIODE)
    }

    /// Après une lecture finie à `maintenant` : quand lire la suivante, et les lignes du journal — le premier
    /// bulletin, une tempête qui s'annonce, une panne (une seule ligne jusqu'au retour), le retour.
    fn apres(&mut self, lu: &Result<Bulletin, String>, maintenant: SystemTime) -> Vec<String> {
        let mut lignes = Vec::new();
        match lu {
            Ok(b) => {
                if self.en_panne {
                    lignes.push("Météo : elle se lit de nouveau.".to_string());
                }
                if !self.deja_lu {
                    lignes.push(format!("Météo : {} (open-meteo, relue toutes les 15 min).", b.ligne));
                }
                if let Some(t) = b.tempete
                    && !self.tempete_dite
                {
                    lignes.push(format!(
                        "Météo : tempête possible aujourd'hui — {:.0} cm de neige, rafales de {:.0} km/h.",
                        t.neige_cm, t.rafales_kmh
                    ));
                }
                self.tempete_dite = b.tempete.is_some();
                self.en_panne = false;
                self.deja_lu = true;
                self.reprise = REPRISE;
                self.prochaine = maintenant + PERIODE;
            }
            Err(e) => {
                if !self.en_panne {
                    let minutes = self.reprise.as_secs().div_ceil(60);
                    lignes.push(format!(
                        "Météo : {e} — nouvel essai dans {minutes} min (une seule ligne jusqu'au retour)."
                    ));
                    self.en_panne = true;
                }
                self.prochaine = maintenant + self.reprise;
                self.reprise = (self.reprise * 2).min(PERIODE);
            }
        }
        lignes
    }
}

/// La dernière requête partie, toutes météos confondues.
static DERNIERE_REQUETE: Mutex<Option<Instant>> = Mutex::new(None);

/// Prend la place d'une requête, ou dit combien de temps attendre encore (une par minute au plus).
fn reserver() -> Result<(), Duration> {
    let mut derniere = DERNIERE_REQUETE.lock().unwrap_or_else(PoisonError::into_inner);
    let maintenant = Instant::now();
    match reste_avant(*derniere, maintenant) {
        Some(reste) => Err(reste),
        None => {
            *derniere = Some(maintenant);
            Ok(())
        }
    }
}

/// Ce qu'il faut encore attendre avant une requête, si la dernière est partie il y a moins d'une minute.
fn reste_avant(derniere: Option<Instant>, maintenant: Instant) -> Option<Duration> {
    let ecoule = maintenant.saturating_duration_since(derniere?);
    ENTRE_DEUX_REQUETES.checked_sub(ecoule).filter(|r| !r.is_zero())
}

/// Une lecture, tout de suite : la requête de la page, puis son bulletin.
fn lire_maintenant(lieu: &Lieu) -> Result<Bulletin, String> {
    let (code, corps) = https::get_public(&requete(lieu), DELAI)?;
    bulletin_de(code, &corps, lieu, mois_du_poste())
}

/// Le bulletin d'une réponse, selon son code HTTP ; un refus dit sa raison quand open-meteo la donne
/// (`{"error": true, "reason": "…"}`).
fn bulletin_de(code: u16, corps: &[u8], lieu: &Lieu, mois: u32) -> Result<Bulletin, String> {
    if code == 200 {
        return lire(corps, lieu, mois);
    }
    let dite = serde_json::from_slice::<Value>(corps).ok().and_then(|v| raison(&v)).map(|r| format!(" : {r}"));
    Err(format!("open-meteo répond {code}{}", dite.unwrap_or_default()))
}

/// La raison d'un refus d'open-meteo, courte et sans caractère de contrôle : elle va au journal.
fn raison(v: &Value) -> Option<String> {
    let r: String = v.get("reason")?.as_str()?.chars().filter(|c| !c.is_control()).take(200).collect();
    Some(r.trim().to_string()).filter(|r| !r.is_empty())
}

/// Un code WMO : un entier positif (open-meteo écrit « 3 » ; « 3.0 » passe aussi).
fn entier(v: &Value) -> Option<u32> {
    let n = v.as_u64().or_else(|| v.as_f64().filter(|f| f.fract() == 0.0 && *f >= 0.0).map(|f| f as u64))?;
    u32::try_from(n).ok()
}

/// La ligne de la page (cdc:571), « MONTRÉAL · 5 °C · COUVERT » : la ville et le libellé en capitales, la température
/// arrondie comme `Math.round`. Sans ville, elle commence à la température ; sans libellé (un code que la table n'a
/// pas), elle s'arrête à la température, où la page laissait un « · » final.
fn ligne(ville: &str, temperature: f64, code: u32) -> String {
    let ville = ville.trim();
    let mut morceaux = Vec::with_capacity(3);
    if !ville.is_empty() {
        morceaux.push(ville.to_uppercase());
    }
    morceaux.push(format!("{} °C", arrondi(temperature)));
    if let Some(l) = libelle(code) {
        morceaux.push(l.to_uppercase());
    }
    morceaux.join(" · ")
}

/// `Math.round` de JavaScript : la demie monte (-2,5 donne -2, où `f64::round` donnerait -3), et un entier n'a pas de
/// zéro négatif (-0,2 s'écrit « 0 », jamais « -0 »).
pub fn arrondi(x: f64) -> i64 {
    let bas = x.floor();
    (if x - bas >= 0.5 { bas + 1.0 } else { bas }) as i64
}

/// Le libellé d'un code WMO : la table de la page, telle quelle (cdc:567-568). Elle n'a pas 56 ni 57 (la bruine
/// verglaçante).
pub fn libelle(code: u32) -> Option<&'static str> {
    Some(match code {
        0 => "Ciel dégagé",
        1 => "Plutôt dégagé",
        2 => "Partiellement nuageux",
        3 => "Couvert",
        45 => "Brouillard",
        48 => "Brouillard givrant",
        51 | 53 => "Bruine",
        55 => "Bruine forte",
        61 => "Pluie légère",
        63 => "Pluie",
        65 => "Pluie forte",
        66 | 67 => "Pluie verglaçante",
        71 => "Neige légère",
        73 => "Neige",
        75 => "Neige forte",
        77 => "Grains de neige",
        80 | 81 => "Averses",
        82 => "Fortes averses",
        85 => "Averses de neige",
        86 => "Fortes averses de neige",
        95 => "Orage",
        96 | 99 => "Orage et grêle",
        _ => return None,
    })
}

/// Une valeur dans une requête : tout sauf les lettres, les chiffres et « -._~ » s'écrit %XX, octet par octet.
fn encoder(s: &str) -> String {
    let mut sortie = String::with_capacity(s.len());
    for o in s.bytes() {
        if o.is_ascii_alphanumeric() || b"-._~".contains(&o) {
            sortie.push(char::from(o));
        } else {
            let _ = write!(sortie, "%{o:02X}");
        }
    }
    sortie
}

/// Le mois du poste, de 1 à 12 : la page prend celui de l'ordinateur (`new Date().getMonth()`, de 0 à 11). Illisible
/// (jamais), 0 : hors de la saison des tempêtes.
fn mois_du_poste() -> u32 {
    journal::date().get(5..7).and_then(|m| m.parse().ok()).unwrap_or(0)
}

#[cfg(test)]
mod essais {
    use serde_json::json;

    use super::*;

    /// Une réponse d'open-meteo, de la forme qu'il rend (relevée le 24 octobre 2026 pour Montréal), aux valeurs
    /// inventées : un matin de janvier, de la neige forte.
    const REPONSE: &str = r#"{
        "latitude": 45.5, "longitude": -73.57, "generationtime_ms": 0.05, "utc_offset_seconds": -18000,
        "timezone": "America/Toronto", "timezone_abbreviation": "GMT-5", "elevation": 36.0,
        "current_units": {"time": "iso8601", "interval": "seconds", "temperature_2m": "°C", "weather_code": "wmo code"},
        "current": {"time": "2027-01-15T08:15", "interval": 900, "temperature_2m": -7.6, "weather_code": 75},
        "daily_units": {"time": "iso8601", "snowfall_sum": "cm", "wind_gusts_10m_max": "km/h"},
        "daily": {"time": ["2027-01-15"], "snowfall_sum": [18.20], "wind_gusts_10m_max": [61.9]}
    }"#;

    fn montreal() -> Lieu {
        Lieu { ville: "Montréal".into(), latitude: 45.50, longitude: -73.57, fuseau: FUSEAU.into() }
    }

    /// La même réponse, avec ces valeurs.
    fn reponse(temperature: f64, code: u32, neige: f64, rafales: f64) -> Vec<u8> {
        let mut v: Value = serde_json::from_str(REPONSE).unwrap();
        v["current"]["temperature_2m"] = json!(temperature);
        v["current"]["weather_code"] = json!(code);
        v["daily"]["snowfall_sum"] = json!([neige]);
        v["daily"]["wind_gusts_10m_max"] = json!([rafales]);
        serde_json::to_vec(&v).unwrap()
    }

    /// Une heure quelconque, pour les horloges des essais.
    fn instant() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
    }

    #[test]
    fn une_reponse_se_lit_comme_dans_la_page() {
        let b = lire(REPONSE.as_bytes(), &montreal(), 1).unwrap();
        assert_eq!(b.ligne, "MONTRÉAL · -8 °C · NEIGE FORTE");
        assert_eq!((b.code, b.temperature, b.il_neige), (75, -7.6, true));
        assert_eq!((b.neige_cm, b.rafales_kmh), (18.2, 61.9));
        assert_eq!(b.tempete, Some(Tempete { neige_cm: 18.2, rafales_kmh: 61.9 }));
        // En octobre, la même neige n'est pas une tempête : sa saison va de novembre à avril.
        assert_eq!(lire(REPONSE.as_bytes(), &montreal(), 10).unwrap().tempete, None);
        // Un jour de prévision, sans ses températures : elles manquent.
        let jour = Jour { date: "2027-01-15".into(), max: None, min: None, code: None, precipitations: None };
        assert_eq!(b.jours, [jour]);
    }

    #[test]
    fn trois_jours_de_prevision() {
        let mut v: Value = serde_json::from_str(REPONSE).unwrap();
        v["daily"] = json!({
            "time": ["2027-01-15", "2027-01-16", "2027-01-17"],
            "snowfall_sum": [18.2, 0.0, 0.0], "wind_gusts_10m_max": [61.9, 30.0, 25.0],
            "temperature_2m_max": [-4.2, -9.0, 1.5], "temperature_2m_min": [-12.0, -18.4, -6.0],
            "weather_code": [75, 0, 3], "precipitation_probability_max": [90, null, 10]
        });
        let b = lire(&serde_json::to_vec(&v).unwrap(), &montreal(), 1).unwrap();
        assert_eq!(b.jours.len(), 3);
        let demain = &b.jours[1];
        assert_eq!((demain.date.as_str(), demain.max, demain.min), ("2027-01-16", Some(-9.0), Some(-18.4)));
        assert_eq!((demain.code, demain.precipitations), (Some(0), None), "une valeur nulle manque");
        assert_eq!(b.tempete, Some(Tempete { neige_cm: 18.2, rafales_kmh: 61.9 }), "la tempête reste celle du jour");
    }

    #[test]
    fn la_table_de_la_page() {
        let l = |code| ligne("Montréal", 5.0, code);
        assert_eq!(l(0), "MONTRÉAL · 5 °C · CIEL DÉGAGÉ");
        assert_eq!(l(1), "MONTRÉAL · 5 °C · PLUTÔT DÉGAGÉ");
        assert_eq!(l(3), "MONTRÉAL · 5 °C · COUVERT");
        assert_eq!(l(48), "MONTRÉAL · 5 °C · BROUILLARD GIVRANT");
        assert_eq!(l(67), "MONTRÉAL · 5 °C · PLUIE VERGLAÇANTE");
        assert_eq!(l(86), "MONTRÉAL · 5 °C · FORTES AVERSES DE NEIGE");
        assert_eq!(l(99), "MONTRÉAL · 5 °C · ORAGE ET GRÊLE");
        // 56 et 57 ne sont pas dans la table : la ligne s'arrête à la température.
        assert_eq!(l(56), "MONTRÉAL · 5 °C");
        let connus: Vec<u32> = (0..=255).filter(|&c| libelle(c).is_some()).collect();
        let page = [0, 1, 2, 3, 45, 48, 51, 53, 55, 61, 63, 65, 66, 67, 71, 73, 75, 77, 80, 81, 82, 85, 86, 95, 96, 99];
        assert_eq!(connus, page);
        // Sans ville, la ligne commence à la température.
        assert_eq!(ligne("  ", 5.0, 3), "5 °C · COUVERT");
    }

    #[test]
    fn la_temperature_s_arrondit_comme_en_javascript() {
        let cas = [
            (-0.2, 0),
            (-0.5, 0),
            (-2.5, -2),
            (2.5, 3),
            (-7.6, -8),
            (4.49, 4),
            (0.499_999_999_999_999_94, 0),
            (-12.5, -12),
        ];
        for (t, attendu) in cas {
            assert_eq!(arrondi(t), attendu, "{t}");
        }
        assert_eq!(ligne("Montréal", -0.2, 0), "MONTRÉAL · 0 °C · CIEL DÉGAGÉ");
        assert_eq!(ligne("Montréal", -14.5, 71), "MONTRÉAL · -14 °C · NEIGE LÉGÈRE");
    }

    #[test]
    fn la_tempete_de_novembre_a_avril() {
        for mois in [11, 12, 1, 2, 3, 4] {
            assert!(tempete_de(mois, 5.0, 0.0).is_some(), "{mois}");
            assert!(tempete_de(mois, 0.0, 70.0).is_some(), "{mois}");
        }
        for mois in [5, 6, 7, 8, 9, 10, 0, 13] {
            assert_eq!(tempete_de(mois, 30.0, 120.0), None, "{mois}");
        }
        assert_eq!(tempete_de(1, 4.9, 69.9), None);
        assert_eq!(tempete_de(2, 12.4, 80.6), Some(Tempete { neige_cm: 12.4, rafales_kmh: 80.6 }));
    }

    #[test]
    fn il_neige_pour_les_codes_de_la_page() {
        let neige: Vec<u32> =
            (0..=99).filter(|&c| lire(&reponse(-3.0, c, 0.0, 0.0), &montreal(), 10).unwrap().il_neige).collect();
        assert_eq!(neige, [71, 73, 75, 77, 85, 86]);
    }

    #[test]
    fn le_lieu_vient_de_la_famille() {
        let famille = json!({ "personnes": [], "meteo": {
            "ville": "Montréal", "latitude": 45.50, "longitude": -73.57, "fuseau": "America/Toronto" } });
        assert_eq!(lieu_de(&famille), Some(montreal()));
        // Sans fuseau (ou un fuseau vide), celui de la page ; la ville se lit sans ses blancs.
        let l = lieu_de(&json!({ "meteo": { "ville": "Montréal", "latitude": 45.5, "longitude": -73.57 } }));
        assert_eq!(l, Some(montreal()));
        let vide = json!({ "meteo": { "ville": " Montréal ", "latitude": 45.5, "longitude": -73.57, "fuseau": " " } });
        assert_eq!(lieu_de(&vide), Some(montreal()));
        // Des coordonnées en chaînes, à la française aussi ; sans ville.
        let l = lieu_de(&json!({ "meteo": { "latitude": "45,50", "longitude": " -73.57 " } })).unwrap();
        assert_eq!((l.latitude, l.longitude, l.ville.as_str()), (45.5, -73.57, ""));
        // Sans coordonnées lisibles, pas de lieu.
        for f in [
            Value::Null,
            json!({}),
            json!({ "meteo": null }),
            json!({ "meteo": "Montréal" }),
            json!({ "meteo": { "ville": "Montréal" } }),
            json!({ "meteo": { "latitude": 45.5 } }),
            json!({ "meteo": { "latitude": 91, "longitude": 0 } }),
            json!({ "meteo": { "latitude": 0, "longitude": -180.5 } }),
            json!({ "meteo": { "latitude": "nord", "longitude": 0 } }),
            json!({ "meteo": { "latitude": "NaN", "longitude": 0 } }),
            json!({ "meteo": { "latitude": true, "longitude": 0 } }),
        ] {
            assert_eq!(lieu_de(&f), None, "{f}");
        }
    }

    #[test]
    fn la_requete_ne_porte_que_le_lieu() {
        let attendue = concat!(
            "https://api.open-meteo.com/v1/forecast?latitude=45.50&longitude=-73.57",
            "&current=temperature_2m,weather_code&daily=snowfall_sum,wind_gusts_10m_max,temperature_2m_max,",
            "temperature_2m_min,weather_code,precipitation_probability_max&forecast_days=3",
            "&timezone=America%2FToronto"
        );
        assert_eq!(requete(&montreal()), attendue);
        // Des coordonnées précises (celles d'une adresse) partent au centième, comme dans la page.
        let r = requete(&Lieu { latitude: 45.508_811, longitude: -73.554_167, ..montreal() });
        assert!(r.contains("?latitude=45.51&longitude=-73.55&"), "{r}");
        // La ville n'y va jamais ; le fuseau s'encode.
        assert!(!r.to_lowercase().contains("montr"), "{r}");
        let r = requete(&Lieu { fuseau: "Europe/Paris & co".into(), ..montreal() });
        assert!(r.ends_with("&timezone=Europe%2FParis%20%26%20co"), "{r}");
    }

    #[test]
    fn une_reponse_douteuse_ne_donne_pas_de_bulletin() {
        let essai = |json: &[u8]| lire(json, &montreal(), 1);
        assert!(essai(b"<html>").unwrap_err().contains("JSON"));
        let e = essai(br#"{"error": true, "reason": "Invalid timezone"}"#).unwrap_err();
        assert_eq!(e, "open-meteo refuse la demande : Invalid timezone");
        assert!(essai(br#"{"daily": {}}"#).unwrap_err().contains("température"));
        assert!(essai(br#"{"current": {"temperature_2m": 3.5}}"#).unwrap_err().contains("code"));
        assert!(essai(br#"{"current": {"temperature_2m": 3.5, "weather_code": -1}}"#).is_err());
        assert!(essai(br#"{"current": {"temperature_2m": "3", "weather_code": 3}}"#).is_err());
        // « 3.0 » est un code ; sans jour, ni neige ni rafales, donc pas de tempête, même en janvier.
        let b = essai(br#"{"current": {"temperature_2m": 3.5, "weather_code": 3.0}}"#).unwrap();
        assert_eq!(b.ligne, "MONTRÉAL · 4 °C · COUVERT");
        assert_eq!((b.neige_cm, b.rafales_kmh, b.tempete), (0.0, 0.0, None));
        // Une valeur nulle compte pour zéro, comme `|| 0`.
        let b = essai(
            br#"{"current": {"temperature_2m": 1, "weather_code": 0},
                           "daily": {"snowfall_sum": [null], "wind_gusts_10m_max": []}}"#,
        )
        .unwrap();
        assert_eq!((b.neige_cm, b.rafales_kmh), (0.0, 0.0));
    }

    #[test]
    fn un_refus_dit_son_code_et_sa_raison() {
        // Le retour de ligne de la raison (un « \r\n » du JSON) ne passe pas au journal.
        let refus = br#"{"error": true, "reason": "Latitude must be in range of -90 to 90.\r\nGiven: 91.0."}"#;
        let e = bulletin_de(400, refus, &montreal(), 1).unwrap_err();
        assert_eq!(e, "open-meteo répond 400 : Latitude must be in range of -90 to 90.Given: 91.0.");
        assert_eq!(bulletin_de(429, b"Too Many Requests", &montreal(), 1).unwrap_err(), "open-meteo répond 429");
        assert_eq!(bulletin_de(502, b"", &montreal(), 1).unwrap_err(), "open-meteo répond 502");
        assert!(bulletin_de(200, REPONSE.as_bytes(), &montreal(), 1).is_ok());
    }

    #[test]
    fn un_bulletin_reste_bon_une_heure() {
        let t = instant();
        assert!(frais(t, t));
        assert!(frais(t, t + Duration::from_secs(59 * 60)));
        assert!(frais(t, t + VALIDITE));
        assert!(!frais(t, t + VALIDITE + Duration::from_secs(1)));
        // L'horloge a reculé : il reste bon jusqu'à la lecture suivante.
        assert!(frais(t, t - Duration::from_secs(3600)));
    }

    #[test]
    fn une_panne_se_dit_une_fois() {
        let t0 = instant();
        let mut s = Suivi::nouveau(t0);
        assert!(s.a_lire(t0));
        let lu = lire(REPONSE.as_bytes(), &montreal(), 10);
        let panne: Result<Bulletin, String> = Err("le nom du serveur est introuvable (pas de réseau ?)".into());
        // Le premier bulletin se dit ; le suivant, dans 15 min.
        assert_eq!(s.apres(&lu, t0), ["Météo : MONTRÉAL · -8 °C · NEIGE FORTE (open-meteo, relue toutes les 15 min)."]);
        assert!(!s.a_lire(t0 + Duration::from_secs(14 * 60)));
        assert!(s.a_lire(t0 + PERIODE));
        assert!(s.apres(&lu, t0 + PERIODE).is_empty());
        // Une panne : une ligne, puis rien jusqu'au retour ; les essais reprennent vite, puis s'espacent.
        let mut t = t0 + PERIODE * 2;
        let (mut dites, mut attentes) = (Vec::new(), Vec::new());
        for _ in 0..6 {
            dites.extend(s.apres(&panne, t));
            let attente = s.prochaine.duration_since(t).unwrap();
            attentes.push(attente.as_secs() / 60);
            t += attente;
            assert!(s.a_lire(t));
        }
        let dite = "Météo : le nom du serveur est introuvable (pas de réseau ?) — nouvel essai dans 1 min (une seule \
                    ligne jusqu'au retour).";
        assert_eq!(dites, [dite]);
        assert_eq!(attentes, [1, 2, 4, 8, 15, 15]);
        // Le retour se dit, une fois ; la période reprend, et la panne suivante repart à une minute.
        assert_eq!(s.apres(&lu, t), ["Météo : elle se lit de nouveau."]);
        assert_eq!(s.prochaine.duration_since(t).unwrap(), PERIODE);
        assert_eq!(s.apres(&panne, t).len(), 1);
        assert_eq!(s.prochaine.duration_since(t).unwrap(), REPRISE);
    }

    #[test]
    fn une_tempete_se_dit_quand_elle_s_annonce() {
        let t = instant();
        let mut s = Suivi::nouveau(t);
        let calme = lire(&reponse(-3.0, 3, 0.0, 20.0), &montreal(), 1);
        let tempete = lire(&reponse(-3.0, 75, 12.4, 40.0), &montreal(), 1);
        assert_eq!(s.apres(&calme, t).len(), 1);
        assert_eq!(
            s.apres(&tempete, t),
            ["Météo : tempête possible aujourd'hui — 12 cm de neige, rafales de 40 km/h."]
        );
        assert!(s.apres(&tempete, t).is_empty());
        // Elle passe sans bruit, et se redit si elle revient.
        assert!(s.apres(&calme, t).is_empty());
        assert_eq!(s.apres(&tempete, t).len(), 1);
    }

    #[test]
    fn l_heure_de_lire() {
        let t = instant();
        let mut s = Suivi::nouveau(t);
        s.prochaine = t + Duration::from_secs(5 * 60);
        assert!(!s.a_lire(t));
        assert!(s.a_lire(t + Duration::from_secs(5 * 60)));
        assert!(s.a_lire(t + Duration::from_secs(6 * 60)));
        // L'horloge a reculé d'une heure : la lecture prévue serait dans 1 h 05 ; elle se fait tout de suite.
        assert!(s.a_lire(t - Duration::from_secs(3600)));
    }

    #[test]
    fn une_requete_par_minute_au_plus() {
        let t = Instant::now();
        assert_eq!(reste_avant(None, t), None);
        assert_eq!(reste_avant(Some(t), t), Some(ENTRE_DEUX_REQUETES));
        assert_eq!(reste_avant(Some(t), t + Duration::from_secs(45)), Some(Duration::from_secs(15)));
        assert_eq!(reste_avant(Some(t), t + ENTRE_DEUX_REQUETES), None);
        assert_eq!(reste_avant(Some(t), t + Duration::from_secs(600)), None);
    }
}
