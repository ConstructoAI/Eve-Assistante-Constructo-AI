//! Le tableau de bord du thème « Constructo AI » ([`super::theme`]) sous sa bannière, sur le modèle demandé (ses
//! cartes, pas ses couleurs) : à gauche le choix « Tout · Entreprise · Famille » (le réglage
//! `vue_du_tableau`, que garde `reglages.json`), à droite l'heure de la dernière lecture de l'entreprise (« Mis à jour
//! à 15 h 40 ») et le bouton qui la relit ; une rangée de tuiles de chiffres (six au plus) ; puis des cartes en
//! listes, en maçonnerie sur trois colonnes ([`theme::maconnerie`]), les plus importantes d'abord ([`rang`]). « Tout »
//! mêle l'entreprise et la maison ; « Entreprise » et « Famille » ne gardent que l'une. Une carte sans données ne se
//! montre pas. La personne déplace les cartes à la souris : leur ordre est le sien (le réglage `ordre_du_tableau`,
//! posé sur l'ordre d'office : [`theme::ranger`]), jusqu'à « Ordre d'office ».
//!
//! Chaque carte a la même grammaire ([`theme::Liste`]) : une tête (la pastille de son icône, son titre, une pilule de
//! compte teintée selon l'urgence), des rangées séparées d'un filet (un mot de tête en couleur — rouge pour ce qui est
//! en retard ou presse —, le texte, un montant à droite, une ligne de détail grise), un pied gris. La maison les donne
//! (`activite\*.js`, [`crate::centre::Centre::listes`]) ; l'entreprise aussi : la dernière lecture de son ERP,
//! Constructo AI, que tient Eve dans son dossier local (`entreprise.json`, relue par le fil du centre quand elle
//! change : [`crate::centre::Centre::entreprise`]), chaque champ facultatif — ses alertes, l'agenda de l'ERP, les
//! factures en retard, les projets en cours, les devis en attente, la balance âgée, le pipeline, la banque.
//!
//! Avec l'entreprise (et avec la maison seule quand Constructo AI n'est pas permis à cette Eve), les cartes en ligne :
//! l'état de Claude, de GitHub et de Render d'après leurs pages d'état, les erreurs des journaux de Render
//! ([`crate::services`]), les montants de Stripe et ses derniers paiements ([`crate::stripe`]) — que lisent les suivis
//! de la fenêtre, chez qui a posé leurs clés : sans clé, pas de carte. Ce qui presse monte (un incident, des erreurs
//! dans la dernière heure, un paiement échoué) ; une lecture manquée se dit dans sa carte ; un clic sur un service
//! ouvre sa page d'état.
//!
//! Les états de l'entreprise : Constructo AI pas permis à cette Eve (ni le choix ni l'entreprise : la famille seule,
//! comme avant), pas reliée (une carte invite à se relier, le geste `constructo_connecter`), la page de connexion
//! ouverte, la première lecture (une roue : le serveur endormi se réveille en une trentaine de secondes), une relecture
//! (les chiffres d'avant restent, rien ne clignote), une lecture qui n'a pas abouti (son bandeau et « Réessayer », le
//! geste `entreprise_lire`), plus reliée avec des chiffres d'avant (pâlis, et « Se reconnecter »).
//!
//! Les montants à la québécoise (« 12 000 $ » ; « 1,25 M$ » dans une tuile). Les barres suivent les règles des
//! tableaux de bord : fines, les bouts arrondis, deux points de blanc entre les parts d'une barre empilée, chaque
//! valeur aussi écrite en clair (une bulle au survol ne fait que la redire). La balance âgée dit le retard par une
//! rampe chaude d'une seule teinte (de l'orange au rouge foncé : plus c'est vieux, plus c'est foncé), vérifiée par le
//! validateur de palettes en ordinal (la luminosité monotone, des marches de 0,06 au moins, le bout clair à 2,26:1 sur
//! le blanc).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui::{
    self, Align, Align2, Color32, FontFamily, FontId, Layout, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind,
    UiBuilder, pos2, vec2,
};
use serde_json::Value;

use super::theme::{
    self, Action, BoutonErp, Chiffre, Clic, ECART, LigneDeMenu, Liste, PetitChiffre, Rangee, erp, pictos,
};
use super::{Fenetre, ICONES, SEMI, TENUE, graphiques};
use crate::meteo::{self, Bulletin};
use crate::reglages::VUES_DU_TABLEAU;
use crate::services::{Erreurs, Etat, Niveau, Releve, Sorte};
use crate::stripe::{Issue, Montants, Paiement};
use crate::{ambiance, centre};

/// Les rangs des cartes du tableau de bord, une seule échelle pour l'entreprise et la maison : la plus petite d'abord,
/// en haut de la maçonnerie. Ce qui presse, puis la journée et le temps qu'il fera, puis l'argent qui attend, puis le
/// reste.
pub(crate) mod rang {
    /// Ce qui est en alerte à la maison (la tempête, la cueillette, un courriel de l'école).
    pub const ALERTES_DE_LA_MAISON: u8 = 0;
    /// Les alertes de l'entreprise, quand l'une est haute ; et l'invitation à relier Constructo AI.
    pub const ALERTES_HAUTES: u8 = 1;
    /// Ce qui presse en ligne : un incident chez Claude, GitHub ou Render, des erreurs de Render dans la dernière
    /// heure, un paiement de Stripe échoué depuis un jour.
    pub const EN_LIGNE_QUI_PRESSE: u8 = 2;
    /// Ce qu'on a épinglé (les notes de la personne, les cartes d'Eve) : juste après ce qui presse en ligne — elles
    /// s'ajoutent les dernières —, sous les yeux.
    pub const EPINGLES: u8 = 2;
    pub const DECISION: u8 = 3;
    pub const JOURNEE: u8 = 4;
    pub const METEO: u8 = 5;
    pub const AGENDA_DE_L_ENTREPRISE: u8 = 6;
    pub const FACTURES_EN_RETARD: u8 = 7;
    /// Stripe : ses montants, puis ses derniers paiements.
    pub const STRIPE: u8 = 8;
    pub const CHANTIERS: u8 = 9;
    pub const ECHEANCES: u8 = 10;
    /// Les soumissions aux clients (ou, sans elles, les devis en attente).
    pub const SOUMISSIONS: u8 = 11;
    pub const COMMANDES: u8 = 12;
    pub const HEURES: u8 = 13;
    pub const COURRIELS: u8 = 14;
    pub const EQUIPE: u8 = 15;
    pub const FACTURES_RECUES: u8 = 16;
    /// Les alertes de l'entreprise, aucune haute (ou aucune).
    pub const ALERTES: u8 = 17;
    /// En ligne, à surveiller : des erreurs de Render dans la journée (aucune dans la dernière heure), une maintenance
    /// en cours, une lecture de Render manquée.
    pub const EN_LIGNE_A_SURVEILLER: u8 = 18;
    pub const BALANCE: u8 = 19;
    pub const PIPELINE: u8 = 20;
    pub const BANQUE: u8 = 21;
    /// Les services en ligne quand tout va, et Render sans erreur : sous l'entreprise.
    pub const EN_LIGNE: u8 = 22;
    pub const EPICERIE: u8 = 23;
    pub const QUESTIONS: u8 = 24;
    pub const ENCAISSE: u8 = 25;
    pub const COMPTE: u8 = 26;
    pub const CHIFFRES: u8 = 27;
}

// ---- La vue -----------------------------------------------------------------------------------------------------

/// Ce que montre le tableau de bord, dans l'ordre de [`VUES_DU_TABLEAU`] : l'entreprise et la famille, ou l'une des
/// deux.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Vue {
    Tout,
    Entreprise,
    Famille,
}

/// Les vues et leurs icônes, dans l'ordre de [`VUES_DU_TABLEAU`] (qui a leurs noms).
const VUES: [(Vue, char); 3] =
    [(Vue::Tout, pictos::TOUT), (Vue::Entreprise, pictos::ENTREPRISE), (Vue::Famille, pictos::FAMILLE)];

/// Où la fenêtre garde la vue choisie d'un clic, le temps que le chef la publie.
const VUE_TENUE: &str = "vue-du-tableau-choisie-d-un-clic";

impl Vue {
    /// La vue de ce nom (celui du réglage) ; un nom inconnu : tout.
    fn de(nom: &str) -> Vue {
        VUES_DU_TABLEAU.iter().position(|v| v.0 == nom).map_or(Vue::Tout, |k| VUES[k].0)
    }

    fn rang(self) -> usize {
        VUES.iter().position(|v| v.0 == self).unwrap_or(0)
    }
}

/// Les chiffres que montrent les tuiles de chaque vue, dans l'ordre : les six premiers qui ont une valeur. Leurs clés :
/// celles de [`chiffres_de_l_entreprise`] et de [`crate::centre::Centre::chiffres`].
const TUILES_DE_TOUT: [&str; 13] = [
    "calendrier", "projets", "recevoir", "retard", "devis", "courriels", "heures", "echeances", "pipeline", "rappels",
    "encaisse", "epicerie", "questions",
];
const TUILES_DE_L_ENTREPRISE: [&str; 11] = [
    "projets", "recevoir", "retard", "devis", "heures", "pipeline", "encaisse", "a_commander", "livraisons", "banque",
    "employes",
];
const TUILES_DE_LA_FAMILLE: [&str; 6] = ["calendrier", "courriels", "rappels", "echeances", "epicerie", "questions"];

/// Les tuiles d'une rangée, au plus.
const SIX: usize = 6;

// ---- La liaison à Constructo AI, et la fraîcheur des chiffres -----------------------------------------------------

/// Où en est la liaison d'Eve à Constructo AI, d'après ce qu'en publie le chef (`constructo` : `permis`, `etat`) et ce
/// que dit la lecture (`permis` faux : pas permis).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Liaison {
    /// Constructo AI n'est pas permis à cette Eve : le tableau n'a pas d'entreprise.
    Absente,
    /// Pas reliée (ou plus) : la carte qui invite à se relier.
    NonReliee,
    /// La page de connexion est ouverte, ou Eve se relie.
    EnCours,
    Reliee,
}

impl Liaison {
    fn de(constructo: &Value, lecture: Option<&Result<Arc<Value>, String>>) -> Liaison {
        let refusee = lecture.and_then(|l| l.as_ref().ok()).is_some_and(|v| v["permis"].as_bool() == Some(false));
        if refusee || !constructo["permis"].as_bool().unwrap_or(false) {
            return Liaison::Absente;
        }
        match constructo["etat"].as_str() {
            Some("connectee") => Liaison::Reliee,
            Some("en_cours") => Liaison::EnCours,
            _ => Liaison::NonReliee,
        }
    }
}

/// L'heure de l'image, pour dire l'âge d'une lecture : le jour local (son numéro depuis 1970), l'heure locale en
/// secondes civiles (comptées comme une heure universelle), et l'heure universelle ; leur écart est le fuseau du poste.
#[derive(Clone, Copy, Debug)]
struct Maintenant {
    jour: i64,
    local: i64,
    unix: i64,
}

impl Maintenant {
    fn du_poste() -> Maintenant {
        let local = secondes_civiles(&crate::journal::date_et_heure()).unwrap_or(0);
        let unix = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
        Maintenant { jour: local.div_euclid(86_400), local, unix }
    }

    /// Le jour de l'image, « 2026-10-26 ».
    fn aujourdhui(&self) -> String {
        centre::date_du_numero(self.jour)
    }

    /// Quand a eu lieu un instant Unix (au fuseau du poste à cette image), dit comme une lecture ([`quand_dit`]) :
    /// « à 9 h 38 », « hier à 22 h 10 », « le 24 oct. à 9 h » — l'heure d'un seul tenant, qu'un bout de ligne ne coupe
    /// pas entre « 9 h » et « 38 ».
    fn quand(&self, unix: i64) -> String {
        let q = quand_dit(unix + (self.local - self.unix), self).replace(" h ", "\u{a0}h\u{a0}");
        match q.strip_suffix(" h") {
            Some(debut) => format!("{debut}\u{a0}h"),
            None => q,
        }
    }

    /// L'âge d'une lecture faite à `lu_a` (« 2026-10-26T13:45:00-04:00 », « …Z », ou l'heure du poste sans fuseau), en
    /// secondes (jamais négatif : une horloge en avance dit « à l'instant »), et son heure locale en secondes civiles ;
    /// `None` pour une heure qui ne se lit pas.
    fn age(&self, lu_a: &str) -> Option<(i64, i64)> {
        let (civiles, fuseau) = instant_ecrit(lu_a)?;
        let (age, local) = match fuseau {
            Some(decalage) => {
                let unix = civiles - decalage;
                (self.unix - unix, unix + (self.local - self.unix))
            }
            None => (self.local - civiles, civiles),
        };
        Some((age.max(0), local))
    }
}

/// « 2026-10-26 15:10:02 », « 2026-10-26T15:10 » → les secondes depuis le 1er janvier 1970 de cette heure, comptée
/// comme une heure universelle ; `None` si ce n'en est pas une.
fn secondes_civiles(texte: &str) -> Option<i64> {
    let (date, heure) = (texte.get(..10)?, texte.get(11..)?);
    let jour = centre::numero_du_jour(date)?;
    let mut p = heure.split(':');
    let h: i64 = p.next()?.get(..2)?.parse().ok()?;
    let m: i64 = p.next()?.get(..2)?.parse().ok()?;
    let s: i64 = match p.next() {
        Some(s) => s.get(..2)?.parse().ok()?,
        None => 0,
    };
    ((0..24).contains(&h) && (0..60).contains(&m) && (0..61).contains(&s))
        .then_some(jour * 86_400 + h * 3600 + m * 60 + s)
}

/// Une heure écrite comme le fait la lecture de l'ERP (ISO 8601 : « 2026-10-26T13:45:00-04:00 », « …Z ») ou sans
/// fuseau : ses secondes civiles ([`secondes_civiles`]) et son fuseau en secondes (`None` : l'heure du poste).
fn instant_ecrit(texte: &str) -> Option<(i64, Option<i64>)> {
    let t = texte.trim();
    let o = t.as_bytes();
    let n = o.len();
    let (corps, fuseau) = if let Some(c) = t.strip_suffix(['Z', 'z']) {
        (c, Some(0))
    } else if n >= 22 && matches!(o[n - 6], b'+' | b'-') && o[n - 3] == b':' {
        let signe = if o[n - 6] == b'-' { -1 } else { 1 };
        let h: i64 = t.get(n - 5..n - 3)?.parse().ok()?;
        let m: i64 = t.get(n - 2..)?.parse().ok()?;
        (t.get(..n - 6)?, Some(signe * (h * 3600 + m * 60)))
    } else {
        (t, None)
    };
    Some((secondes_civiles(corps)?, fuseau))
}

/// Quand une lecture a été faite, à dire après « Mis à jour » : « à 15 h 40 » aujourd'hui, « hier à 18 h 05 », « le 24
/// oct. à 9 h » ; `local` : son heure locale en secondes civiles.
fn quand_dit(local: i64, m: &Maintenant) -> String {
    let jour = local.div_euclid(86_400);
    let s = local.rem_euclid(86_400);
    let heure = centre::heure_parlee(&format!("{:02}:{:02}", s / 3600, s / 60 % 60));
    if jour == m.jour {
        format!("à {heure}")
    } else if jour == m.jour - 1 {
        format!("hier à {heure}")
    } else {
        format!("le {} à {heure}", centre::date_courte(&centre::date_du_numero(jour)))
    }
}

/// Quand la lecture `v` a été faite (son `lu_a`), à dire, et son âge en secondes ; `None` sans heure lisible.
fn lue_quand(v: &Value, m: &Maintenant) -> Option<(i64, String)> {
    let (age, local) = m.age(v["lu_a"].as_str()?)?;
    Some((age, quand_dit(local, m)))
}

/// Ce que dit la fraîcheur des chiffres, à droite du choix de la vue : son point (rien quand tout est à jour ; une roue
/// quand Eve lit), son texte.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Point {
    Aucun,
    Ambre,
    Rouge,
    Gris,
    Roue,
}

#[derive(Debug, PartialEq)]
struct Fraicheur {
    point: Point,
    texte: String,
}

/// Des chiffres de plus de 30 minutes se disent d'un moment, un point ambre devant : Eve les relit toutes les dix
/// minutes environ.
const PERIMES: i64 = 30 * 60;

/// La fraîcheur des chiffres de l'entreprise : la liaison d'abord, puis ce que dit la lecture (son `etat`, son
/// `lu_a`) — « Mis à jour à 15 h 40 », comme le modèle.
fn fraicheur(liaison: Liaison, lecture: Option<&Result<Arc<Value>, String>>, m: &Maintenant) -> Fraicheur {
    let f = |point, texte: &str| Fraicheur { point, texte: texte.to_string() };
    match liaison {
        Liaison::Absente => return f(Point::Aucun, ""),
        Liaison::EnCours => return f(Point::Roue, "Connexion à Constructo AI…"),
        Liaison::NonReliee => return f(Point::Gris, "Constructo AI n'est pas relié"),
        Liaison::Reliee => {}
    }
    let v = match lecture {
        None => return f(Point::Roue, "Première lecture de Constructo AI…"),
        Some(Err(_)) => return f(Point::Rouge, "Lecture illisible"),
        Some(Ok(v)) => v,
    };
    let quand = lue_quand(v, m);
    match v["etat"].as_str().unwrap_or("a_jour") {
        "en_cours" | "non_connecte" if !a_des_chiffres(v) => f(Point::Roue, "Première lecture de Constructo AI…"),
        "en_cours" => f(Point::Roue, "Mise à jour…"),
        "erreur" => match quand {
            Some((_, q)) => f(Point::Rouge, &format!("Lecture manquée · mis à jour {q}")),
            None => f(Point::Rouge, "Lecture manquée"),
        },
        _ => match quand {
            Some((age, q)) if age < PERIMES => f(Point::Aucun, &format!("Mis à jour {q}")),
            Some((_, q)) => f(Point::Ambre, &format!("Mis à jour {q}")),
            None => f(Point::Aucun, "À jour"),
        },
    }
}

// ---- L'entreprise en chiffres et en listes ------------------------------------------------------------------------

/// La lecture a des chiffres à montrer : ses indicateurs, ses heures, sa balance âgée, ou l'une de ses listes.
fn a_des_chiffres(v: &Value) -> bool {
    let liste = |cle: &str| v[cle].as_array().is_some_and(|l| !l.is_empty());
    let objet = |cle: &str| v[cle].as_object().is_some_and(|o| !o.is_empty());
    ["kpis", "balance_agee", "heures"].into_iter().any(objet)
        || [
            "alertes", "projets", "factures_en_retard", "pipeline", "agenda", "devis", "banque", "soumissions",
            "factures_recues", "commandes",
        ]
        .into_iter()
        .any(liste)
}

/// Un nombre de la lecture : un nombre JSON, ou un texte qui en est un (« 1234,50 »).
fn nombre(v: &Value, cle: &str) -> Option<f64> {
    let x = v.get(cle)?;
    x.as_f64().or_else(|| x.as_str()?.trim().replace(',', ".").parse().ok()).filter(|n: &f64| n.is_finite())
}

fn texte_de<'a>(v: &'a Value, cle: &str) -> &'a str {
    v.get(cle).and_then(Value::as_str).map_or("", str::trim)
}

/// Un montant à la québécoise, arrondi au dollar : « 12 000 $ » (une espace fine entre les milliers, une espace
/// insécable avant le dollar).
pub(crate) fn dollars(v: f64) -> String {
    format!("{}\u{a0}$", centre::milliers(v))
}

/// Un montant de tuile, court : « 1,25 M$ » dès le million (deux décimales au plus, sans zéro de trop), sinon
/// [`dollars`].
fn dollars_court(v: f64) -> String {
    if v.abs() < 999_995.0 {
        return dollars(v);
    }
    let millions = format!("{:.2}", v / 1_000_000.0);
    let millions = millions.trim_end_matches('0').trim_end_matches('.').replace('.', ",");
    format!("{millions}\u{a0}M$")
}

/// Un compte, au singulier sous deux : « 1 facture », « 4 factures » (une espace insécable : le nombre ne se sépare
/// pas de ce qu'il compte au bout d'une ligne).
fn compte(n: f64, un: &str, plusieurs: &str) -> String {
    format!("{}\u{a0}{}", centre::milliers(n), if n.round() >= 2.0 { plusieurs } else { un })
}

/// Des heures dites : « 3,5 h », « 45 h », « 36,25 h », « 1 204,5 h » (une espace fine entre les milliers).
fn heures_dites(h: f64) -> String {
    let centiemes = (h * 100.0).round() as i64;
    let entier = centre::milliers((centiemes / 100) as f64);
    let reste = format!("{:02}", (centiemes % 100).abs());
    match reste.trim_end_matches('0') {
        "" => format!("{entier}\u{a0}h"),
        fraction => format!("{entier},{fraction}\u{a0}h"),
    }
}

/// Les chiffres de l'entreprise pour les tuiles (ses indicateurs, `kpis`, et ses heures), chacun sous sa clé :
/// « projets » (en cours), « recevoir », « retard » (en alerte s'il y en a), « devis » (en attente), « heures » (de la
/// semaine), « pipeline », « encaisse » (en 30 jours), « a_commander », « livraisons » (prévues), « banque » (le
/// solde), « employes » (actifs). Seulement ceux que la lecture donne.
fn chiffres_de_l_entreprise(v: &Value) -> Vec<(&'static str, Chiffre)> {
    let kpis = &v["kpis"];
    let n = |cle: &str| nombre(kpis, cle);
    let chiffre = |valeur: String, etiquette: &str, icone: char, teinte: erp::Teinte, alerte: bool| Chiffre {
        valeur,
        etiquette: etiquette.to_string(),
        icone,
        teinte,
        alerte,
    };
    let selon = |v: f64, un: &'static str, plusieurs: &'static str| if v.round() >= 2.0 { plusieurs } else { un };
    let mut c = Vec::new();
    if let Some(p) = n("projets_en_cours") {
        let etiquette = selon(p, "chantier en cours", "chantiers en cours");
        c.push(("projets", chiffre(centre::milliers(p), etiquette, pictos::PROJETS, erp::BLEU, false)));
    }
    if let Some(a) = n("a_recevoir") {
        c.push(("recevoir", chiffre(dollars_court(a), "à recevoir", pictos::RECEVOIR, erp::SARCELLE, false)));
    }
    if let Some(r) = n("en_retard") {
        let etiquette = match n("factures_en_retard") {
            Some(f) if r > 0.0 => format!("en retard · {}", compte(f, "facture", "factures")),
            _ => "en retard".to_string(),
        };
        c.push(("retard", chiffre(dollars_court(r), &etiquette, pictos::RETARD, erp::ORANGE, r > 0.0)));
    }
    if let Some(d) = n("devis_en_attente") {
        c.push(("devis", chiffre(centre::milliers(d), "devis en attente", pictos::DEVIS, erp::INDIGO, false)));
    }
    if let Some(h) = nombre(&v["heures"], "cette_semaine") {
        let etiquette = "heures cette semaine";
        c.push(("heures", chiffre(heures_dites(h), etiquette, pictos::HEURES, erp::SARCELLE, false)));
    }
    if let Some(p) = n("pipeline") {
        let etiquette = match n("opportunites_ouvertes") {
            Some(o) => format!("au pipeline · {}", compte(o, "opportunité", "opportunités")),
            None => "au pipeline des ventes".to_string(),
        };
        c.push(("pipeline", chiffre(dollars_court(p), &etiquette, pictos::PIPELINE, erp::VIOLET, false)));
    }
    if let Some(e) = n("encaissements_30_jours") {
        let etiquette = "encaissés ces 30 jours";
        c.push(("encaisse", chiffre(dollars_court(e), etiquette, pictos::ENCAISSE, erp::VERT, false)));
    }
    if let Some(b) = n("bons_a_commander") {
        let etiquette = selon(b, "bon à commander", "bons à commander");
        c.push(("a_commander", chiffre(centre::milliers(b), etiquette, pictos::COMMANDES, erp::AMBRE, false)));
    }
    if let Some(l) = n("livraisons_prevues") {
        let etiquette = selon(l, "livraison prévue", "livraisons prévues");
        c.push(("livraisons", chiffre(centre::milliers(l), etiquette, pictos::COMMANDES, erp::VERT, false)));
    }
    if let Some(b) = n("solde_banque") {
        c.push(("banque", chiffre(dollars_court(b), "en banque", pictos::ENCAISSE, erp::SARCELLE, false)));
    }
    if let Some(e) = n("employes_actifs") {
        let etiquette = selon(e, "employé actif", "employés actifs");
        c.push(("employes", chiffre(centre::milliers(e), etiquette, pictos::PERSONNE, erp::GRIS, false)));
    }
    c
}

/// Les rangées que montre une carte de l'entreprise ; au-delà, « … et 3 autres ».
const RANGEES: usize = 6;

/// La gravité d'une alerte, pour les ranger (la plus grave d'abord) : haute, moyenne, info.
fn rang_de_gravite(g: &str) -> u8 {
    match g {
        "haute" => 0,
        "moyenne" => 1,
        _ => 2,
    }
}

/// Le mot de tête d'une rangée, dans la couleur vive d'une teinte, assombrie au besoin pour se lire sur le blanc.
fn tete(teinte: erp::Teinte) -> Color32 {
    theme::lisible(teinte.vif[1])
}

/// La couleur d'un retard de `jours`, pour le mot de tête d'une facture : l'ambre sous un mois, l'orange sous deux,
/// puis le rouge.
fn couleur_du_retard(jours: f64) -> Color32 {
    match jours {
        j if j <= 30.0 => tete(erp::AMBRE),
        j if j <= 60.0 => tete(erp::ORANGE),
        _ => erp::DANGER_ENCRE,
    }
}

/// Le pied d'une liste coupée : « … et 3 autres ».
fn reste(n: usize, un: &str, plusieurs: &str) -> String {
    if n == 0 { String::new() } else { format!("… et {n} {}", if n > 1 { plusieurs } else { un }) }
}

/// Les tranches de la balance âgée (leur clé dans la lecture, leur nom) et leur couleur : ce qui est à venir en bleu,
/// puis le retard dans la rampe chaude (orange-400, rouge-500, rouge-700, rouge-900 de Tailwind), du plus récent au
/// plus vieux.
const TRANCHES: [(&str, &str, Color32); 5] = [
    ("a_venir", "À venir", erp::PRIMAIRE),
    ("0_30", "1 à 30 jours", Color32::from_rgb(0xFB, 0x92, 0x3C)),
    ("31_60", "31 à 60 jours", Color32::from_rgb(0xEF, 0x44, 0x44)),
    ("61_90", "61 à 90 jours", Color32::from_rgb(0xB9, 0x1C, 0x1C)),
    ("plus_de_90", "Plus de 90 jours", Color32::from_rgb(0x7F, 0x1D, 0x1D)),
];

/// Les cartes de l'entreprise en listes, chacune avec son rang ([`rang`]) : ses alertes, l'agenda de l'ERP, les
/// factures en retard, les projets en cours, les devis en attente, la balance âgée, le pipeline, la banque — seulement
/// celles que la lecture nourrit.
fn listes_de_l_entreprise(v: &Value, m: &Maintenant) -> Vec<(u8, Liste)> {
    let liste = |cle: &str| v[cle].as_array().filter(|l| !l.is_empty());
    let kpis = &v["kpis"];
    let mut listes = Vec::new();
    if let Some(a) = v["alertes"].as_array() {
        listes.push(liste_des_alertes(a));
    }
    if let Some(a) = liste("agenda") {
        listes.push((rang::AGENDA_DE_L_ENTREPRISE, liste_de_l_agenda(a, m)));
    }
    if let Some(f) = liste("factures_en_retard") {
        listes.push((rang::FACTURES_EN_RETARD, liste_des_factures(f, nombre(kpis, "en_retard"))));
    }
    if let Some(p) = liste("projets") {
        listes.push((rang::CHANTIERS, liste_des_chantiers(p, nombre(kpis, "projets_en_cours"))));
    }
    // Les soumissions (brouillons, validées, envoyées, en attente) comprennent les devis en attente : une carte pour
    // les deux.
    if let Some(s) = liste("soumissions") {
        listes.push((rang::SOUMISSIONS, liste_des_soumissions(s)));
    } else if let Some(d) = liste("devis") {
        listes.push((rang::SOUMISSIONS, liste_des_devis(d, nombre(kpis, "devis_en_attente"))));
    }
    if let Some(c) = liste("commandes") {
        listes.push((rang::COMMANDES, liste_des_commandes(c, kpis)));
    }
    if v["heures"].as_object().is_some_and(|h| !h.is_empty()) {
        listes.push((rang::HEURES, liste_des_heures(&v["heures"])));
        if let Some(l) = liste_de_l_equipe(&v["heures"]) {
            listes.push((rang::EQUIPE, l));
        }
    }
    if let Some(f) = liste("factures_recues") {
        listes.push((rang::FACTURES_RECUES, liste_des_factures_recues(f)));
    }
    if v["balance_agee"].as_object().is_some_and(|b| !b.is_empty()) {
        let encaisse = nombre(kpis, "encaissements_30_jours");
        listes.push((rang::BALANCE, liste_de_la_balance(&v["balance_agee"], encaisse)));
    }
    if let Some(p) = liste("pipeline") {
        listes.push((rang::PIPELINE, liste_du_pipeline(p, nombre(kpis, "pipeline"))));
    }
    if let Some(b) = liste("banque") {
        listes.push((rang::BANQUE, liste_de_la_banque(b)));
    }
    listes
}

/// Les alertes de l'ERP, la plus grave d'abord : chacune son mot de tête (sa sorte — « Factures », « Stock »,
/// « Conformité », « Banque », « Heures » —, sinon sa gravité) dans la couleur de sa gravité (le rouge d'une alerte
/// haute, l'ambre d'une moyenne, le bleu d'une information), et son texte ; leur nombre dans la pilule, de la teinte
/// de la plus grave. Aucune : « Aucune », en vert. En haut de la page quand l'une est haute.
fn liste_des_alertes(alertes: &[Value]) -> (u8, Liste) {
    let mut a: Vec<(u8, &str, &str)> = alertes
        .iter()
        .map(|x| (rang_de_gravite(texte_de(x, "gravite")), texte_de(x, "sorte"), texte_de(x, "texte")))
        .filter(|x| !x.2.is_empty())
        .collect();
    a.sort_by_key(|x| x.0);
    let teinte = match a.first() {
        Some((0, _, _)) => erp::ROUGE,
        Some(_) => erp::AMBRE,
        None => erp::VERT,
    };
    let mut l = Liste::nouvelle("Alertes", pictos::ALERTES, teinte);
    if a.is_empty() {
        l = l.pilule("Aucune", erp::VERT);
        l.rangees.push(Rangee::nouvelle("", erp::TEXTE, "Rien d'urgent dans Constructo AI."));
        return (rang::ALERTES, l);
    }
    l = l.pilule(a.len().to_string(), teinte);
    for (gravite, sorte, texte) in a.iter().take(RANGEES) {
        let mot = match (*sorte, gravite) {
            ("factures", _) => "Factures",
            ("stock", _) => "Stock",
            ("conformite", _) => "Conformité",
            ("banque", _) => "Banque",
            ("heures", _) => "Heures",
            (_, 0) => "Urgent",
            (_, 1) => "À surveiller",
            _ => "Info",
        };
        let couleur = match gravite {
            0 => erp::DANGER_ENCRE,
            1 => tete(erp::AMBRE),
            _ => erp::PRIMAIRE,
        };
        // Un texte qui commence par son mot de tête (« Heures manquantes le … ») ne le redit pas : le mot de tête,
        // en couleur, commence la phrase.
        let suite = texte
            .get(mot.len()..)
            .filter(|s| s.starts_with(' ') && texte[..mot.len()].to_lowercase() == mot.to_lowercase());
        match suite {
            Some(s) => l.rangees.push(Rangee::nouvelle(&texte[..mot.len()], couleur, s.trim_start())),
            None => l.rangees.push(Rangee::nouvelle(mot, couleur, texte)),
        }
    }
    l.pied = reste(a.len().saturating_sub(RANGEES), "autre", "autres");
    let rang = if teinte == erp::ROUGE { rang::ALERTES_HAUTES } else { rang::ALERTES };
    (rang, l)
}

/// L'agenda de l'entreprise (les notes partagées du calendrier de l'ERP, d'aujourd'hui aux jours qui viennent) : la
/// date en mot de tête (« Aujourd'hui », « Demain », « jeu. 29 oct. »), la note ; leur nombre dans la pilule. Ce qui
/// est passé ne se montre pas.
fn liste_de_l_agenda(agenda: &[Value], m: &Maintenant) -> Liste {
    let aujourdhui = m.aujourdhui();
    let notes: Vec<(String, &str)> = agenda
        .iter()
        .filter_map(|a| {
            let date = texte_de(a, "quand").get(..10)?.to_string();
            (centre::jours_entre(&aujourdhui, &date)? >= 0).then_some((date, texte_de(a, "texte")))
        })
        .filter(|(_, t)| !t.is_empty())
        .collect();
    let titre = "Agenda de l'entreprise";
    let mut l = Liste::nouvelle(titre, pictos::AGENDA, erp::INDIGO).pilule(notes.len().to_string(), erp::INDIGO);
    for (date, texte) in notes.iter().take(RANGEES) {
        let (mot, couleur) = match centre::jours_entre(&aujourdhui, date) {
            Some(0) => ("Aujourd'hui".to_string(), erp::PRIMAIRE),
            Some(1) => ("Demain".to_string(), tete(erp::INDIGO)),
            _ => (centre::jour_court(date), tete(erp::INDIGO)),
        };
        l.rangees.push(Rangee::nouvelle(&mot, couleur, texte));
    }
    if notes.is_empty() {
        l.rangees.push(Rangee::nouvelle("", erp::TEXTE, "Rien au calendrier de l'entreprise."));
    }
    l.pied = reste(notes.len().saturating_sub(RANGEES), "autre note", "autres notes");
    l
}

/// Les factures en retard, la plus vieille d'abord : le client en mot de tête, de la couleur de son retard (l'ambre
/// sous un mois, l'orange sous deux, puis le rouge), le numéro, le solde à droite ; ses jours de retard et son échéance
/// en détail ; tout le retard dans la pilule, en rouge.
fn liste_des_factures(factures: &[Value], total: Option<f64>) -> Liste {
    let total = total.unwrap_or_else(|| factures.iter().filter_map(|f| nombre(f, "solde")).sum());
    let mut l = Liste::nouvelle("Factures en retard", pictos::RETARD, erp::ROUGE)
        .pilule(dollars(total), erp::ROUGE)
        .actions(&[Action::Relance]);
    for f in factures.iter().take(RANGEES) {
        let jours = nombre(f, "jours").unwrap_or(0.0);
        let client = Some(texte_de(f, "client")).filter(|c| !c.is_empty()).unwrap_or("Client");
        let numero = texte_de(f, "numero");
        let numero = if numero.is_empty() { String::new() } else { format!("n° {numero}") };
        let echeance = texte_de(f, "echeance");
        let echue =
            if echeance.is_empty() { String::new() } else { format!("échue le {}", centre::date_courte(echeance)) };
        let retard = compte(jours, "jour de retard", "jours de retard");
        let mut r = Rangee::nouvelle(client, couleur_du_retard(jours), &numero).detail(&[retard, echue]);
        if let Some(s) = nombre(f, "solde") {
            r = r.a_droite(dollars(s));
        }
        l.rangees.push(r);
    }
    l.pied = reste(factures.len().saturating_sub(RANGEES), "autre facture", "autres factures");
    l
}

/// Les chantiers en cours (les projets « En cours » de l'ERP), du plus récent au plus ancien : leur numéro en mot de
/// tête quand il est court (sinon en détail), leur nom et leur adresse, leur valeur à droite ; leur client, leur ville
/// et leur date de fin en détail ; leur nombre dans la pilule.
fn liste_des_chantiers(projets: &[Value], combien: Option<f64>) -> Liste {
    let combien = combien.map_or(projets.len(), |c| c.round().max(0.0) as usize);
    let mut l =
        Liste::nouvelle("Chantiers en cours", pictos::PROJETS, erp::BLEU).pilule(combien.to_string(), erp::BLEU);
    for p in projets.iter().take(RANGEES) {
        let numero = texte_de(p, "numero");
        let court = numero.chars().count() <= 10;
        let nom = Some(texte_de(p, "nom")).filter(|n| !n.is_empty()).unwrap_or(numero);
        // L'adresse du chantier, quand son nom ne la dit pas déjà : « Agrandissement (12 rue des Exemples) ».
        let adresse = texte_de(p, "adresse");
        let nom =
            if adresse.is_empty() || nom.contains(adresse) { nom.to_string() } else { format!("{nom} ({adresse})") };
        let (debut, fin) = (texte_de(p, "debut"), texte_de(p, "fin"));
        let dates = match (debut.is_empty(), fin.is_empty()) {
            (_, false) => format!("jusqu'au {}", centre::date_courte(fin)),
            (false, true) => format!("depuis le {}", centre::date_courte(debut)),
            _ => String::new(),
        };
        let long = if !court && numero != nom { numero } else { "" };
        let mot = if court { numero } else { "" };
        let detail = [texte_de(p, "client"), texte_de(p, "ville"), &dates, long];
        let mut r = Rangee::nouvelle(mot, tete(erp::BLEU), &nom).detail(&detail);
        if let Some(v) = nombre(p, "valeur") {
            r = r.a_droite(dollars(v));
        }
        l.rangees.push(r);
    }
    l.pied = reste(combien.saturating_sub(projets.len().min(RANGEES)), "autre chantier", "autres chantiers");
    l
}

/// Un statut de devis de l'ERP (« Brouillon », « Valide », « Envoye », « En attente »…), dit : « brouillon »,
/// « validée », « envoyée », « en attente » — au féminin, une soumission.
fn statut_de_soumission(statut: &str) -> String {
    match statut.trim().to_lowercase().as_str() {
        "valide" | "validé" | "validée" => "validée".to_string(),
        "envoye" | "envoyé" | "envoyée" => "envoyée".to_string(),
        "accepte" | "accepté" | "acceptée" => "acceptée".to_string(),
        autre => autre.to_string(),
    }
}

/// Les soumissions aux clients (brouillons, validées, envoyées, en attente), la dernière modifiée d'abord : leur numéro
/// en mot de tête, leur nom, leur valeur à droite ; le client, leur statut et le jour de leur dernière modification
/// en détail ; leur nombre dans la pilule.
fn liste_des_soumissions(soumissions: &[Value]) -> Liste {
    let titre = "Soumissions client";
    let combien = soumissions.len().to_string();
    let mut l =
        Liste::nouvelle(titre, pictos::DEVIS, erp::INDIGO).pilule(combien, erp::INDIGO).actions(&[Action::Suivi]);
    for s in soumissions.iter().take(RANGEES) {
        let modifie = texte_de(s, "modifie");
        let modifie =
            if modifie.is_empty() { String::new() } else { format!("modifiée le {}", centre::jour_court(modifie)) };
        let detail = [texte_de(s, "client").to_string(), statut_de_soumission(texte_de(s, "statut")), modifie];
        let mut r = Rangee::nouvelle(texte_de(s, "numero"), tete(erp::INDIGO), texte_de(s, "nom")).detail(&detail);
        if let Some(v) = nombre(s, "valeur") {
            r = r.a_droite(dollars(v));
        }
        l.rangees.push(r);
    }
    l.pied = reste(soumissions.len().saturating_sub(RANGEES), "autre soumission", "autres soumissions");
    l
}

/// Les commandes et les livraisons : ce qui est à commander (en ambre), ce qui est commandé (en bleu), les livraisons
/// prévues (en vert) — chacune son mot de tête, son texte (ou son numéro), son montant à droite ; son numéro, son
/// fournisseur, son chantier et sa date en détail. Dans la pilule, ce qui est à commander (en ambre), sinon combien
/// il y en a.
fn liste_des_commandes(commandes: &[Value], kpis: &Value) -> Liste {
    let ordre = |c: &&Value| match texte_de(c, "sorte") {
        "a_commander" => 0,
        "commande" => 1,
        _ => 2,
    };
    let mut triees: Vec<&Value> = commandes.iter().collect();
    triees.sort_by_key(ordre);
    let a_commander = nombre(kpis, "bons_a_commander")
        .unwrap_or_else(|| commandes.iter().filter(|c| texte_de(c, "sorte") == "a_commander").count() as f64);
    let (pilule, teinte) = if a_commander > 0.0 {
        (format!("{} à commander", centre::milliers(a_commander)), erp::AMBRE)
    } else {
        (commandes.len().to_string(), erp::ORANGE)
    };
    let mut l = Liste::nouvelle("Commandes et livraisons", pictos::COMMANDES, erp::ORANGE).pilule(pilule, teinte);
    for c in triees.iter().take(RANGEES) {
        let date = texte_de(c, "date");
        let (mot, couleur, quand) = match texte_de(c, "sorte") {
            "a_commander" => ("À commander", tete(erp::AMBRE), "pour le"),
            "commande" => ("Commandé", tete(erp::BLEU), "livraison prévue le"),
            _ => ("Livraison", tete(erp::VERT), "le"),
        };
        let heure = texte_de(c, "heure");
        let quand = match (date.is_empty(), heure.is_empty()) {
            (true, _) => String::new(),
            (false, true) => format!("{quand} {}", centre::jour_court(date)),
            (false, false) => format!("{quand} {} à {}", centre::jour_court(date), centre::heure_parlee(heure)),
        };
        let (texte, numero) = match texte_de(c, "texte") {
            "" => (texte_de(c, "numero"), ""),
            t => (t, texte_de(c, "numero")),
        };
        let detail = [numero, texte_de(c, "fournisseur"), texte_de(c, "projet"), &quand];
        let mut r = Rangee::nouvelle(mot, couleur, texte).detail(&detail);
        if let Some(m) = nombre(c, "montant") {
            r = r.a_droite(dollars(m));
        }
        l.rangees.push(r);
    }
    l.pied = reste(commandes.len().saturating_sub(RANGEES), "autre", "autres");
    l
}

/// Les heures de la semaine (les pointages de l'ERP) : la semaine en cours et ce qui y est travaillé, la semaine
/// passée, ce qui est saisi pour des jours à venir (à vérifier), les heures manquantes (en rouge : le jour et qui),
/// puis les heures de la semaine par chantier ; combien d'employés ont pointé dans la pilule.
fn liste_des_heures(h: &Value) -> Liste {
    let mut l = Liste::nouvelle("Heures de la semaine", pictos::HEURES, erp::SARCELLE);
    if let Some(n) = nombre(h, "employes_actifs") {
        l = l.pilule(compte(n, "employé actif", "employés actifs"), erp::SARCELLE);
    }
    let semaine = texte_de(h, "semaine");
    let mot = if semaine.is_empty() {
        "Cette semaine".to_string()
    } else {
        format!("Semaine du {}", centre::date_courte(semaine))
    };
    if let Some(c) = nombre(h, "cette_semaine") {
        let saisie = texte_de(h, "derniere_saisie");
        let derniere = match (saisie.get(..10), saisie.get(11..16)) {
            (Some(d), Some(t)) => format!("dernière saisie : {} à {}", centre::jour_court(d), centre::heure_parlee(t)),
            (Some(d), None) => format!("dernière saisie : {}", centre::jour_court(d)),
            _ => String::new(),
        };
        let texte = format!("{} travaillées jusqu'à aujourd'hui", heures_dites(c));
        l.rangees.push(Rangee::nouvelle(&mot, tete(erp::SARCELLE), &texte).detail(&[derniere]));
    }
    if let Some(p) = nombre(h, "semaine_passee") {
        l.rangees.push(Rangee::nouvelle(
            "Semaine passée",
            tete(erp::SARCELLE),
            &format!("{} travaillées", heures_dites(p)),
        ));
    }
    if let Some(a) = nombre(h, "a_venir").filter(|a| *a > 0.0) {
        let texte = format!("{} saisies pour des jours à venir", heures_dites(a));
        l.rangees.push(Rangee::nouvelle("À vérifier", tete(erp::AMBRE), &texte));
    }
    let manquantes = &h["manquantes"];
    let qui: Vec<&str> =
        manquantes["employes"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::trim).collect();
    if !qui.is_empty() {
        let jour = texte_de(manquantes, "jour");
        let le = if jour.is_empty() { String::new() } else { format!("le {} ", centre::jour_court(jour)) };
        let texte = format!("{le}pour : {}", qui.join(", "));
        l.rangees.push(Rangee::nouvelle("Heures manquantes", erp::DANGER_ENCRE, &texte));
    }
    // Les heures de la semaine par chantier : chacun sa barre (d'une seule couleur : la longueur les compare).
    let par_projet: Vec<&Value> = h["par_projet"].as_array().into_iter().flatten().collect();
    let plus = par_projet.iter().filter_map(|p| nombre(p, "heures")).fold(0.0, f64::max);
    for p in par_projet.iter().take(RANGEES) {
        let numero = texte_de(p, "numero");
        let mot = if numero.chars().count() <= 10 { numero } else { "" };
        let nom = texte_de(p, "nom");
        let mut r = Rangee::nouvelle(mot, tete(erp::BLEU), nom);
        if let Some(n) = nombre(p, "heures") {
            r = r.a_droite(heures_dites(n));
            let part = if plus > 0.0 { (n / plus) as f32 } else { 0.0 };
            let bulle = format!("{nom} : {} cette semaine", heures_dites(n));
            r.mesure = Some(theme::Mesure::Longueur { part, couleur: erp::SARCELLE.vif[1], bulle });
        }
        l.rangees.push(r);
    }
    if !par_projet.is_empty() {
        l.pied = match par_projet.len().saturating_sub(RANGEES) {
            0 => "Les heures de cette semaine, par chantier".to_string(),
            n => format!(
                "Les heures de cette semaine, par chantier (et {})",
                compte(n as f64, "autre chantier", "autres chantiers")
            ),
        };
    }
    l
}

/// Les heures de l'équipe (les gars de chantier), un employé par rangée : son nom en mot de tête — rouge s'il lui
/// manque des heures —, ses heures de cette semaine et de la semaine passée ; ses jours de cette semaine (« lun. 9 ·
/// mar. 9,5 »), ce qu'il a saisi pour des jours à venir et s'il est au travail en détail ; combien ils sont dans la
/// pilule (tous ceux qui ont pointé : la lecture n'en donne que vingt). Rien sans employés.
fn liste_de_l_equipe(h: &Value) -> Option<Liste> {
    let employes = h["employes"].as_array().filter(|e| !e.is_empty())?;
    let manquants: Vec<&str> =
        h["manquantes"]["employes"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::trim).collect();
    let semaine = texte_de(h, "semaine");
    let titre = "Heures de l'équipe";
    let actifs = nombre(h, "employes_actifs").map_or(0, |n| n.round().max(0.0) as usize).max(employes.len());
    let combien = compte(actifs as f64, "employé", "employés");
    let mut l = Liste::nouvelle(titre, pictos::PERSONNE, erp::BLEU).pilule(combien, erp::BLEU);
    for e in employes.iter().take(RANGEES) {
        let nom = texte_de(e, "nom");
        let couleur = if manquants.contains(&nom) { erp::DANGER_ENCRE } else { tete(erp::BLEU) };
        let heures = |cle: &str| nombre(e, cle).map_or("0\u{a0}h".to_string(), heures_dites);
        let texte =
            format!("{} cette semaine · {} la semaine passée", heures("cette_semaine"), heures("semaine_passee"));
        let jours: Vec<String> = e["jours"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|j| semaine.is_empty() || texte_de(j, "jour") >= semaine)
            .filter_map(|j| {
                let jour = texte_de(j, "jour");
                let n = nombre(j, "heures")?;
                let court = centre::jour_court(jour);
                Some(format!(
                    "{} {}",
                    court.split(' ').next().unwrap_or(""),
                    heures_dites(n).trim_end_matches("\u{a0}h")
                ))
            })
            .collect();
        let a_venir = nombre(e, "a_venir")
            .filter(|a| *a > 0.0)
            .map_or(String::new(), |a| format!("{} à venir, à vérifier", heures_dites(a)));
        let au_travail = if e["au_travail"].as_bool() == Some(true) { "au travail" } else { "" };
        l.rangees.push(Rangee::nouvelle(nom, couleur, &texte).detail(&[
            jours.join(" · "),
            a_venir,
            au_travail.to_string(),
        ]));
    }
    l.pied = reste(actifs.saturating_sub(employes.len().min(RANGEES)), "autre employé", "autres employés");
    Some(l)
}

/// Les factures reçues des fournisseurs, la plus récente d'abord : le fournisseur en mot de tête (rouge si elle est en
/// retard), le numéro, ce qui en reste à payer à droite (sinon le montant) ; le chantier, le jour de la facture et où
/// elle en est en détail (« à payer » tant qu'un solde reste) ; leur nombre dans la pilule.
fn liste_des_factures_recues(factures: &[Value]) -> Liste {
    let combien = factures.len().to_string();
    let mut l =
        Liste::nouvelle("Factures reçues", pictos::FACTURES_RECUES, erp::SARCELLE).pilule(combien, erp::SARCELLE);
    for f in factures.iter().take(RANGEES) {
        let numero = texte_de(f, "numero");
        let numero = if numero.is_empty() { String::new() } else { format!("n° {numero}") };
        let date = texte_de(f, "date");
        let date = if date.is_empty() { String::new() } else { format!("du {}", centre::jour_court(date)) };
        let solde = nombre(f, "solde").filter(|s| *s > 0.0);
        let code = texte_de(f, "statut").to_uppercase();
        let statut = match code.as_str() {
            "PAYEE" | "PAYÉE" => "payée",
            "PARTIELLEMENT_PAYEE" => "payée en partie",
            "EN_RETARD" => "en retard",
            "BROUILLON" => "brouillon",
            "ANNULEE" | "ANNULÉE" => "annulée",
            _ if solde.is_some() => "à payer",
            _ => "",
        };
        let detail = [texte_de(f, "projet"), &date, statut];
        let fournisseur = Some(texte_de(f, "fournisseur")).filter(|x| !x.is_empty()).unwrap_or("Fournisseur");
        let couleur = if code == "EN_RETARD" { erp::DANGER_ENCRE } else { tete(erp::SARCELLE) };
        let mut r = Rangee::nouvelle(fournisseur, couleur, &numero).detail(&detail);
        if let Some(s) = solde.or_else(|| nombre(f, "montant")) {
            r = r.a_droite(dollars(s));
        }
        l.rangees.push(r);
    }
    l.pied = reste(factures.len().saturating_sub(RANGEES), "autre facture", "autres factures");
    l
}

/// Les devis qui attendent une réponse, du plus récent au plus ancien : leur numéro en mot de tête, leur nom, leur
/// valeur à droite ; le client, ce qu'ils attendent et depuis quand en détail ; leur nombre dans la pilule.
fn liste_des_devis(devis: &[Value], combien: Option<f64>) -> Liste {
    let combien = combien.map_or(devis.len(), |c| c.round().max(0.0) as usize);
    let mut l = Liste::nouvelle("Devis en attente", pictos::DEVIS, erp::INDIGO)
        .pilule(combien.to_string(), erp::INDIGO)
        .actions(&[Action::Suivi]);
    for d in devis.iter().take(RANGEES) {
        let statut = match texte_de(d, "statut").to_lowercase().as_str() {
            "envoye" | "envoyé" => "envoyé".to_string(),
            autre => autre.to_string(),
        };
        let depuis = texte_de(d, "depuis");
        let depuis =
            if depuis.is_empty() { String::new() } else { format!("depuis le {}", centre::date_courte(depuis)) };
        let detail = [texte_de(d, "client"), &statut, &depuis];
        let mut r = Rangee::nouvelle(texte_de(d, "numero"), tete(erp::INDIGO), texte_de(d, "nom")).detail(&detail);
        if let Some(v) = nombre(d, "valeur") {
            r = r.a_droite(dollars(v));
        }
        l.rangees.push(r);
    }
    l.pied = reste(combien.saturating_sub(devis.len().min(RANGEES)), "autre devis", "autres devis");
    l
}

/// La balance âgée : la barre de ses tranches (le bleu de ce qui est à venir, puis la rampe du retard ; une bulle au
/// survol de chaque part), puis chaque tranche non vide — son carré de couleur, son nom, son montant à droite ; tout le
/// dû dans la pilule ; au pied, ce qui est entré en trente jours.
fn liste_de_la_balance(b: &Value, encaisse: Option<f64>) -> Liste {
    let montants: Vec<f64> = TRANCHES.iter().map(|(cle, _, _)| nombre(b, cle).unwrap_or(0.0).max(0.0)).collect();
    let somme: f64 = montants.iter().sum();
    let total = nombre(b, "total").unwrap_or(somme);
    let mut l = Liste::nouvelle("Balance âgée", pictos::BALANCE, erp::SARCELLE).pilule(dollars(total), erp::SARCELLE);
    for ((_, nom, couleur), montant) in TRANCHES.iter().zip(&montants) {
        let part = if somme > 0.0 { (montant / somme * 100.0).round() } else { 0.0 };
        l.barre.push((*montant, *couleur, format!("{nom} : {} ({part}\u{a0}%)", dollars(*montant))));
        if *montant > 0.0 {
            let mut r = Rangee::nouvelle("", erp::TEXTE, nom).a_droite(dollars(*montant));
            r.carre = Some(*couleur);
            l.rangees.push(r);
        }
    }
    if l.rangees.is_empty() {
        l.rangees.push(Rangee::nouvelle("", erp::TEXTE, "Rien à recevoir."));
    }
    if let Some(e) = encaisse {
        l.pied = format!("{} encaissés ces 30 derniers jours", dollars(e));
    }
    l
}

/// Le pipeline des ventes, ses étapes ouvertes par valeur : le nom de l'étape et son nombre d'opportunités, sa valeur à
/// droite, et une barre de sa valeur (d'une seule couleur : la longueur la dit) ; tout le pipeline dans la pilule.
fn liste_du_pipeline(etapes: &[Value], total: Option<f64>) -> Liste {
    let valeurs: Vec<(&str, Option<f64>, f64)> = etapes
        .iter()
        .map(|e| (texte_de(e, "etape"), nombre(e, "nombre"), nombre(e, "valeur").unwrap_or(0.0).max(0.0)))
        .filter(|(nom, _, _)| !nom.is_empty())
        .collect();
    let plus_grande = valeurs.iter().map(|x| x.2).fold(0.0, f64::max);
    let total = total.unwrap_or_else(|| valeurs.iter().map(|x| x.2).sum());
    let titre = "Pipeline des ventes";
    let mut l = Liste::nouvelle(titre, pictos::PIPELINE, erp::VIOLET).pilule(dollars_court(total), erp::VIOLET);
    for (nom, combien, valeur) in valeurs.iter().take(RANGEES) {
        let mut r = Rangee::nouvelle("", erp::TEXTE, nom).a_droite(dollars(*valeur));
        if let Some(n) = combien {
            let suite = format!(" · {}", compte(*n, "opportunité", "opportunités"));
            r.bouts.push(theme::Bout { couleur: Some(erp::TEXTE_2), ..theme::Bout::texte(suite) });
        }
        let part = if plus_grande > 0.0 { (*valeur / plus_grande) as f32 } else { 0.0 };
        let bulle = format!("{nom} : {}", dollars(*valeur));
        r.mesure = Some(theme::Mesure::Longueur { part, couleur: erp::VIOLET.vif[1], bulle });
        l.rangees.push(r);
    }
    l.pied = reste(valeurs.len().saturating_sub(RANGEES), "autre étape", "autres étapes");
    l
}

/// La banque : chaque compte en mot de tête, son solde (au dernier relevé importé) à droite ; le solde publié de l'ERP
/// et les opérations à classer en détail ; dans la pilule, ce qui est à classer (en ambre), sinon combien de comptes.
fn liste_de_la_banque(comptes: &[Value]) -> Liste {
    let a_classer: f64 = comptes.iter().filter_map(|c| nombre(c, "a_classer")).sum();
    let (pilule, teinte) = if a_classer > 0.0 {
        (format!("{} à classer", centre::milliers(a_classer)), erp::AMBRE)
    } else {
        (compte(comptes.len() as f64, "compte", "comptes"), erp::SARCELLE)
    };
    let mut l = Liste::nouvelle("Banque", pictos::ENCAISSE, erp::SARCELLE).pilule(pilule, teinte);
    for c in comptes.iter().take(RANGEES) {
        let publie = nombre(c, "solde_publie").map_or(String::new(), |s| format!("publié : {}", dollars(s)));
        let classer = nombre(c, "a_classer")
            .filter(|n| *n > 0.0)
            .map_or(String::new(), |n| compte(n, "opération à classer", "opérations à classer"));
        let releve = if nombre(c, "solde").is_none() { "aucun relevé importé".to_string() } else { String::new() };
        let mut r = Rangee::nouvelle(texte_de(c, "nom"), tete(erp::SARCELLE), "").detail(&[publie, classer, releve]);
        if let Some(s) = nombre(c, "solde") {
            r = r.a_droite(dollars(s));
        }
        l.rangees.push(r);
    }
    l.pied = reste(comptes.len().saturating_sub(RANGEES), "autre compte", "autres comptes");
    l
}

/// La météo de la maison et des chantiers, sur trois jours (le bulletin de la fenêtre, d'open-meteo) : chaque jour en
/// mot de tête (« Aujourd'hui », « Demain », « Mercredi »), ses températures (« 12° / -4° »), le temps qu'il fera et
/// la chance de précipitations en détail ; la température de l'heure dans la pilule ; la ville dans le titre. Rien sans
/// prévision.
fn liste_de_la_meteo(b: &Bulletin, ville: &str, m: &Maintenant) -> Option<Liste> {
    if b.jours.is_empty() {
        return None;
    }
    let titre = if ville.trim().is_empty() { "Météo".to_string() } else { format!("Météo à {}", ville.trim()) };
    let icone = match b.code {
        0 | 1 => pictos::SOLEIL,
        2 => pictos::SOLEIL_VOILE,
        _ => pictos::NUAGE,
    };
    let maintenant = format!("{}\u{a0}°C", meteo::arrondi(b.temperature));
    let mut l = Liste::nouvelle(titre, icone, erp::BLEU).pilule(maintenant, erp::BLEU).cle("meteo");
    let aujourdhui = m.aujourdhui();
    let degres = |t: Option<f64>| t.map_or("—".to_string(), |t| format!("{}°", meteo::arrondi(t)));
    for j in b.jours.iter().take(3) {
        let mot = match centre::jours_entre(&aujourdhui, &j.date) {
            Some(0) => "Aujourd'hui".to_string(),
            Some(1) => "Demain".to_string(),
            _ => ambiance::date_longue(&j.date).split(' ').next().map(theme::casse_de_phrase).unwrap_or_default(),
        };
        let temps = j.code.and_then(meteo::libelle).unwrap_or_default().to_string();
        let pluie =
            j.precipitations.filter(|p| *p > 0.0).map_or(String::new(), |p| format!("précipitations {p:.0}\u{a0}%"));
        let r = Rangee::nouvelle(&mot, tete(erp::BLEU), &format!("{} / {}", degres(j.max), degres(j.min)));
        l.rangees.push(r.detail(&[temps, pluie]));
    }
    Some(l)
}

// ---- En ligne : les services, les erreurs de Render, Stripe -------------------------------------------------------

/// Un relevé des services de plus de 15 minutes (ils se relisent toutes les cinq) et des montants de Stripe de plus de
/// 25 minutes (toutes les dix) disent qu'une lecture a été manquée.
const SERVICES_PERIMES: i64 = 15 * 60;
const STRIPE_PERIMES: i64 = 25 * 60;
/// Les composants touchés qu'une rangée nomme ; au-delà, « et 2 autres ».
const COMPOSANTS: usize = 3;
/// Un paiement échoué depuis un jour fait monter sa carte.
const ECHEC_RECENT: i64 = 24 * 3600;

/// Les cartes en ligne, chacune avec son rang : l'état des services et les erreurs de Render (le dernier relevé des
/// services), les montants et les paiements de Stripe. Rien de ce qui n'a pas de relevé : pas de clé, pas encore lu.
fn listes_en_ligne(
    releve: Option<&Releve>,
    stripe: Option<&Result<Montants, String>>,
    m: &Maintenant,
) -> Vec<(u8, Liste)> {
    let mut listes = Vec::new();
    if let Some(r) = releve {
        listes.extend(liste_des_services(r, m));
        listes.extend(liste_des_erreurs(&r.render, r.lu_a, m));
    }
    if let Some(s) = stripe {
        listes.extend(listes_de_stripe(s, m));
    }
    listes
}

/// Un niveau de service, dit ; ses mots courts, ceux d'une pilule (« Tout fonctionne », « Perturbation »).
fn niveau_dit(n: Niveau) -> &'static str {
    match n {
        Niveau::Operationnel => "Opérationnel",
        Niveau::Maintenance => "Maintenance en cours",
        Niveau::Mineur => "Perturbation mineure",
        Niveau::Majeur => "Panne partielle",
        Niveau::Critique => "Panne majeure",
    }
}

fn niveau_court(n: Niveau) -> &'static str {
    match n {
        Niveau::Operationnel => "Tout fonctionne",
        Niveau::Maintenance => "Maintenance",
        Niveau::Mineur => "Perturbation",
        autre => niveau_dit(autre),
    }
}

/// La teinte d'un niveau : le vert de ce qui va, le bleu d'une maintenance, puis l'ambre, l'orange et le rouge.
fn teinte_du_niveau(n: Niveau) -> erp::Teinte {
    match n {
        Niveau::Operationnel => erp::VERT,
        Niveau::Maintenance => erp::BLEU,
        Niveau::Mineur => erp::AMBRE,
        Niveau::Majeur => erp::ORANGE,
        Niveau::Critique => erp::ROUGE,
    }
}

/// Le mot de tête d'un niveau, lisible sur le blanc ; une panne majeure, le rouge foncé de ce qui presse.
fn couleur_du_niveau(n: Niveau) -> Color32 {
    if n == Niveau::Critique { erp::DANGER_ENCRE } else { tete(teinte_du_niveau(n)) }
}

/// Où en est un incident, d'après le mot de sa page : « en cours d'analyse », « cause identifiée », « sous
/// surveillance » ; une maintenance : « prévue », « en cours », « en vérification ». Un mot inconnu passe tel quel.
fn statut_d_incident(statut: &str) -> String {
    let dit = match statut.trim().to_lowercase().as_str() {
        "investigating" => "en cours d'analyse",
        "identified" => "cause identifiée",
        "monitoring" => "sous surveillance",
        "resolved" | "postmortem" => "résolu",
        "scheduled" => "prévue",
        "in_progress" | "in progress" => "en cours",
        "verifying" => "en vérification",
        "completed" => "terminée",
        _ => return statut.trim().replace('_', " "),
    };
    dit.to_string()
}

/// Les composants touchés, chacun nommé une fois : « Actions et Pages ». Ceux d'une même sorte dans plusieurs groupes
/// d'une page (ses régions : « Web Services (Oregon) », « Web Services (Ohio) ») se disent ensemble — « Web Services
/// (Oregon et Ohio) ».
fn composants_dits(composants: &[(String, Niveau)]) -> String {
    let mut sortes: Vec<(&str, Vec<&str>)> = Vec::new();
    for nom in composants.iter().map(|(nom, _)| nom.trim()).filter(|nom| !nom.is_empty()) {
        let (sorte, groupe) = match nom.strip_suffix(')').and_then(|n| n.rsplit_once(" (")) {
            Some((sorte, groupe)) => (sorte.trim(), Some(groupe.trim())),
            None => (nom, None),
        };
        match sortes.iter_mut().find(|(s, _)| *s == sorte) {
            Some((_, groupes)) => {
                if let Some(g) = groupe
                    && !groupes.contains(&g)
                {
                    groupes.push(g);
                }
            }
            None => sortes.push((sorte, groupe.into_iter().collect())),
        }
    }
    let noms: Vec<String> = sortes
        .into_iter()
        .map(|(sorte, groupes)| {
            let groupes: Vec<String> = groupes.into_iter().map(str::to_string).collect();
            if groupes.is_empty() { sorte.to_string() } else { format!("{sorte} ({})", liste_coupee(&groupes)) }
        })
        .collect();
    liste_coupee(&noms)
}

/// Une liste dite, trois au plus : « a », « a et b », « a, b et c », « a, b, c et 2 autres ».
fn liste_coupee(noms: &[String]) -> String {
    if noms.len() <= COMPOSANTS {
        return centre::en_liste(noms);
    }
    let reste = noms.len() - COMPOSANTS;
    format!("{} et {reste} {}", noms[..COMPOSANTS].join(", "), if reste == 1 { "autre" } else { "autres" })
}

/// Depuis quand dure ce qui a commencé à cet instant Unix : « depuis 9 h 12 », « depuis hier à 22 h 10 », « depuis le
/// 24 oct. à 9 h ».
fn depuis_dit(unix: i64, m: &Maintenant) -> String {
    let q = m.quand(unix);
    format!("depuis\u{a0}{}", q.strip_prefix("à ").unwrap_or(&q))
}

/// Le pied d'une carte en ligne : quand elle a été lue (« Mis à jour à 9 h 40 »), après ce qu'elle compte (`quoi`) ;
/// lue il y a plus de `perimes` secondes, qu'une lecture a été manquée.
fn pied_en_ligne(quoi: &str, lu_a: i64, perimes: i64, m: &Maintenant) -> String {
    let q = m.quand(lu_a);
    let lue = if m.unix - lu_a > perimes {
        format!("lecture manquée · mis à jour {q}")
    } else {
        format!("mis à jour {q}")
    };
    if quoi.is_empty() { capitale(&lue) } else { format!("{quoi} · {lue}") }
}

/// Les services en ligne (Claude, GitHub, Render : leurs pages d'état), un par rangée, dans l'ordre de leurs pages :
/// son nom en mot de tête, de la couleur de son niveau ; « Opérationnel », son incident en cours (son nom tel que la
/// page l'écrit, d'ordinaire en anglais) ou ce qui ne va pas ; en détail, la gravité, où en est l'incident, depuis
/// quand, les composants touchés ; une page qui ne se lit plus le dit. Un clic sur une rangée ouvre la page d'état.
/// Dans la pilule, « Tout fonctionne » en vert, sinon les incidents (ou la maintenance), de la teinte du pire. En haut
/// de la page pour un incident, plus bas pendant une maintenance, sous l'entreprise quand tout va. Rien sans page.
fn liste_des_services(r: &Releve, m: &Maintenant) -> Option<(u8, Liste)> {
    if r.statuts.is_empty() {
        return None;
    }
    let lus: Vec<&Etat> = r.statuts.iter().filter_map(|s| s.etat.as_ref().ok()).collect();
    let pire = lus
        .iter()
        .flat_map(|e| e.incidents.iter().map(|i| i.impact).chain([e.niveau]))
        .max()
        .unwrap_or(Niveau::Operationnel);
    let incidents = lus.iter().flat_map(|e| &e.incidents).filter(|i| i.impact != Niveau::Maintenance).count();
    let manquees = r.statuts.len() - lus.len();
    let (pilule, teinte) = if pire >= Niveau::Mineur && incidents > 0 {
        (compte(incidents as f64, "incident", "incidents"), teinte_du_niveau(pire))
    } else if pire > Niveau::Operationnel {
        (niveau_court(pire).to_string(), teinte_du_niveau(pire))
    } else if incidents > 0 {
        // Un incident sans gravité dite (la page enquête encore) : pour information, en bleu.
        (compte(incidents as f64, "incident", "incidents"), erp::BLEU)
    } else if manquees > 0 {
        ("Lecture manquée".to_string(), erp::GRIS)
    } else {
        (niveau_court(Niveau::Operationnel).to_string(), erp::VERT)
    };
    let mut l = Liste::nouvelle("Services en ligne", pictos::SERVICES, teinte).pilule(pilule, teinte);
    for s in &r.statuts {
        let mut rangee = match &s.etat {
            Ok(e) => rangee_d_un_service(&s.nom, e, m),
            Err(raison) => {
                Rangee::nouvelle(&s.nom, erp::TEXTE_2, "Lecture manquée").detail(&[commande_d_un_tenant(raison.trim())])
            }
        };
        rangee.lien = s.page.starts_with("https://").then(|| s.page.clone());
        l.rangees.push(rangee);
    }
    l.pied = pied_en_ligne("", r.lu_a, SERVICES_PERIMES, m);
    let rang = if pire >= Niveau::Mineur {
        rang::EN_LIGNE_QUI_PRESSE
    } else if pire == Niveau::Maintenance || incidents > 0 {
        rang::EN_LIGNE_A_SURVEILLER
    } else {
        rang::EN_LIGNE
    };
    Some((rang, l))
}

/// La rangée d'un service lu : son nom de la couleur du pire (son niveau, l'impact de ses incidents) ; son incident le
/// plus grave (sa page les range ainsi), sa gravité, où il en est, depuis quand, les composants touchés et les autres
/// incidents ; sans incident, ce qui ne va pas, ou « Opérationnel ».
fn rangee_d_un_service(nom: &str, e: &Etat, m: &Maintenant) -> Rangee {
    let pire = e.incidents.iter().map(|i| i.impact).fold(e.niveau, Niveau::max);
    let couleur = couleur_du_niveau(pire);
    let touches = composants_dits(&e.composants);
    let Some(i) = e.incidents.first() else {
        if pire == Niveau::Operationnel {
            return Rangee::nouvelle(nom, couleur, niveau_dit(pire));
        }
        return Rangee::nouvelle(nom, couleur, niveau_dit(pire)).detail(&[touches]);
    };
    let titre = if i.nom.trim().is_empty() { niveau_dit(pire) } else { i.nom.trim() };
    let gravite = match i.impact {
        Niveau::Operationnel => "",
        Niveau::Maintenance => niveau_court(Niveau::Maintenance),
        autre => niveau_dit(autre),
    };
    let depuis = if i.depuis > 0 { depuis_dit(i.depuis, m) } else { String::new() };
    let autres = match e.incidents.len() - 1 {
        0 => String::new(),
        n => format!("{} en cours", compte(n as f64, "autre incident", "autres incidents")),
    };
    let detail = [gravite.to_string(), statut_d_incident(&i.statut), depuis, touches, autres];
    Rangee::nouvelle(nom, couleur, titre).detail(&detail)
}

/// Un compte : « 1 erreur », « 14 erreurs », « 100+ erreurs » (`plus` : davantage que la lecture n'en a compté).
fn compte_lu(n: usize, plus: bool, un: &str, plusieurs: &str) -> String {
    let (nombre, plus) = (centre::milliers(n as f64), if plus { "+" } else { "" });
    format!("{nombre}{plus}\u{a0}{}", if n >= 2 { plusieurs } else { un })
}

/// Les erreurs des journaux de Render sur 24 heures (leurs lignes au niveau « error »), une rangée par service web puis
/// par base, dans l'ordre de Render : son nom en mot de tête — rouge s'il en a eu dans la dernière heure, ambre dans la
/// journée, vert sans erreur —, sa sorte, leur nombre à droite ; en détail, celles de la dernière heure, l'heure de la
/// plus récente et, à part, les réponses HTTP 5xx d'un service web que Render n'a pas mises au niveau « error » (elles
/// colorent aussi son nom) ; puis la ligne de la plus récente erreur (sinon de la plus récente réponse 5xx), à chasse
/// fixe. Dans la pilule, toutes les erreurs (sans erreur, les réponses 5xx), rouges si la dernière heure en a, ambres
/// sinon ; « Aucune » en vert. En haut de la page pour la dernière heure. Une lecture de Render manquée le dit, avec sa
/// raison ; rien sans service ni base sur Render.
fn liste_des_erreurs(render: &Result<Vec<Erreurs>, String>, lu_a: i64, m: &Maintenant) -> Option<(u8, Liste)> {
    let titre = "Erreurs de Render";
    let ressources = match render {
        Ok(r) if r.is_empty() => return None,
        Ok(r) => r,
        Err(raison) => return Some((rang::EN_LIGNE_A_SURVEILLER, lecture_manquee(titre, pictos::ERREURS, raison))),
    };
    let total: usize = ressources.iter().map(|e| e.nombre).sum();
    let reponses: usize = ressources.iter().map(|e| e.reponses_5xx.nombre).sum();
    let recentes: usize = ressources.iter().map(|e| e.derniere_heure + e.reponses_5xx.derniere_heure).sum();
    let (teinte, rang) = match (recentes, total + reponses) {
        (1.., _) => (erp::ROUGE, rang::EN_LIGNE_QUI_PRESSE),
        (0, 1..) => (erp::AMBRE, rang::EN_LIGNE_A_SURVEILLER),
        _ => (erp::VERT, rang::EN_LIGNE),
    };
    let pilule = match (total, reponses) {
        (0, 0) => "Aucune".to_string(),
        (0, r) => compte_lu(r, ressources.iter().any(|e| e.reponses_5xx.plus), "réponse 5xx", "réponses 5xx"),
        (n, _) => compte_lu(n, ressources.iter().any(|e| e.plus), "erreur", "erreurs"),
    };
    let mut l = Liste::nouvelle(titre, pictos::ERREURS, teinte).pilule(pilule, teinte);
    for e in ressources {
        let r5 = &e.reponses_5xx;
        let couleur = match (e.derniere_heure + r5.derniere_heure, e.nombre + r5.nombre) {
            (1.., _) => erp::DANGER_ENCRE,
            (0, 1..) => tete(erp::AMBRE),
            _ => tete(erp::VERT),
        };
        let sorte = match e.sorte {
            Sorte::Web => "Service web",
            Sorte::Base => "Base de données",
            Sorte::Autre => "Service",
        };
        let nombre = format!("{}{}", centre::milliers(e.nombre as f64), if e.plus { "+" } else { "" });
        let recentes = match e.derniere_heure {
            0 => String::new(),
            n => format!("{} dans la dernière heure", centre::milliers(n as f64)),
        };
        let derniere = e.derniere.as_ref().map_or(String::new(), |d| format!("la dernière {}", m.quand(d.quand)));
        let reponses = match (r5.nombre, r5.derniere_heure) {
            (0, _) => String::new(),
            (n, 0) => compte_lu(n, r5.plus, "réponse HTTP 5xx", "réponses HTTP 5xx"),
            (n, h) => format!(
                "{}, dont {h} dans la dernière heure",
                compte_lu(n, r5.plus, "réponse HTTP 5xx", "réponses HTTP 5xx")
            ),
        };
        let mut r =
            Rangee::nouvelle(e.nom.trim(), couleur, sorte).a_droite(nombre).detail(&[recentes, derniere, reponses]);
        let ligne = e.derniere.as_ref().or(r5.derniere.as_ref());
        r.extrait = ligne.map_or(String::new(), |d| d.message.trim().to_string());
        l.rangees.push(r);
    }
    l.pied = pied_en_ligne("Les dernières 24 heures", lu_a, SERVICES_PERIMES, m);
    Some((rang, l))
}

/// Une carte en ligne dont la lecture a été manquée, sans rien d'avant à montrer : sa raison, et qu'Eve réessaiera.
fn lecture_manquee(titre: &str, icone: char, raison: &str) -> Liste {
    let mut l = Liste::nouvelle(titre, icone, erp::AMBRE).pilule("Lecture manquée", erp::AMBRE);
    l.rangees.push(Rangee::nouvelle("", erp::TEXTE, &commande_d_un_tenant(&phrase(raison))));
    l.pied = "Eve réessaiera d'elle-même".to_string();
    l
}

/// Une raison qui donne une commande (« reposer la clé : eve --cle render ») : la commande d'un seul tenant, qu'un bout
/// de ligne ne coupe pas — l'espace devant une option et celle qui la suit deviennent insécables, comme celles qui
/// collent des guillemets à ce qu'ils entourent.
fn commande_d_un_tenant(texte: &str) -> String {
    let mots: Vec<&str> = texte.split(' ').collect();
    let mut lie = String::with_capacity(texte.len());
    for (k, mot) in mots.iter().enumerate() {
        if k > 0 {
            let avant = mots[k - 1];
            let insecable = mot.starts_with("--") || avant.starts_with("--") || avant == "«" || mot.starts_with('»');
            lie.push(if insecable { '\u{a0}' } else { ' ' });
        }
        lie.push_str(mot);
    }
    lie
}

/// Les cartes de Stripe : ses montants ([`liste_de_stripe`]) puis ses derniers paiements ([`liste_des_paiements`]) ;
/// une lecture manquée (sans montants d'une heure au plus), sa raison, à la place des montants. Un compte qui n'a rien
/// à montrer n'a pas de carte.
fn listes_de_stripe(lecture: &Result<Montants, String>, m: &Maintenant) -> Vec<(u8, Liste)> {
    match lecture {
        Err(raison) => vec![(rang::STRIPE, lecture_manquee("Stripe", pictos::PAIEMENTS, raison))],
        Ok(s) if !a_des_montants(s) => Vec::new(),
        Ok(s) => {
            let mut listes = vec![(rang::STRIPE, liste_de_stripe(s, m))];
            listes.extend(liste_des_paiements(&s.paiements, &s.devise, m));
            listes
        }
    }
}

/// Le compte Stripe a-t-il quelque chose à montrer : un paiement, un solde, un abonnement actif, un virement, une
/// facture ouverte ?
fn a_des_montants(s: &Montants) -> bool {
    s.ce_mois.nombre + s.aujourdhui.nombre > 0
        || !s.paiements.is_empty()
        || s.disponible.abs() >= 0.5
        || s.en_attente.abs() >= 0.5
        || s.abonnements.as_ref().is_some_and(|a| a.actifs > 0)
        || s.virement.is_some()
        || s.ouvertes.as_ref().is_some_and(|o| o.nombre > 0)
}

/// Un montant de Stripe, dans la devise du compte : « 8 921 $ » (le dollar canadien, comme le reste du tableau),
/// « 120 $ US », « 75 € », sinon son code (« 300 CHF ») ; arrondi au dollar comme les autres cartes, sauf avec `cents`
/// (un paiement : « 49,99 $ », mais « 50 $ » quand il est rond).
fn montant_stripe(v: f64, devise: &str, cents: bool) -> String {
    let symbole = match devise.trim().to_uppercase().as_str() {
        "" | "CAD" => "$".to_string(),
        "USD" => "$\u{a0}US".to_string(),
        "EUR" => "€".to_string(),
        "GBP" => "£".to_string(),
        autre => autre.to_string(),
    };
    let centimes = (v * 100.0).round() as i64;
    let nombre = if cents && centimes % 100 != 0 {
        let signe = if centimes < 0 { "-" } else { "" };
        format!("{signe}{},{:02}", centre::milliers((centimes.abs() / 100) as f64), centimes.abs() % 100)
    } else {
        centre::milliers(v)
    };
    format!("{nombre}\u{a0}{symbole}")
}

/// Combien de paiements une somme de Stripe compte : « 3 paiements », « aucun paiement », « au moins 100 paiements »
/// (une lecture arrêtée avant la fin).
fn paiements_dits(nombre: usize, plus: bool) -> String {
    match (nombre, plus) {
        (0, false) => "aucun paiement".to_string(),
        (n, false) => compte(n as f64, "paiement", "paiements"),
        (n, true) => format!("au moins {}", compte(n as f64, "paiement", "paiements")),
    }
}

/// Un jour (« 2026-10-29 ») tel qu'on le dit : « aujourd'hui », « demain », « hier », sinon « le jeu. 29 oct. ».
fn jour_dit(date: &str, m: &Maintenant) -> String {
    match centre::jours_entre(&m.aujourdhui(), date) {
        Some(0) => "aujourd'hui".to_string(),
        Some(1) => "demain".to_string(),
        Some(-1) => "hier".to_string(),
        _ => format!("le {}", centre::jour_court(date)),
    }
}

/// Les montants de Stripe : ce qui est entré aujourd'hui et ce mois-ci (les paiements réussis, remboursements ôtés,
/// avant les frais de Stripe), les abonnements actifs et ce qu'ils rapportent par mois, le solde (disponible, et en
/// attente), le prochain virement (sinon le dernier fait), les factures ouvertes et leur dû (en ambre quand elles ont
/// passé leur échéance) ; ce mois-ci dans la pilule ; au pied, quand ils ont été lus. La carte reste à son rang : ce
/// qui doit la faire monter est neuf (un paiement échoué), pas de vieilles factures. Ce qu'une lecture n'a pas pu lire
/// manque.
fn liste_de_stripe(s: &Montants, m: &Maintenant) -> Liste {
    let argent = |v: f64| montant_stripe(v, &s.devise, false);
    let encre = tete(erp::INDIGO);
    let ce_mois = format!("{} ce mois-ci", argent(s.ce_mois.montant));
    let mut l = Liste::nouvelle("Stripe", pictos::PAIEMENTS, erp::INDIGO).pilule(ce_mois, erp::INDIGO);
    for (mot, somme) in [("Aujourd'hui", &s.aujourdhui), ("Ce mois-ci", &s.ce_mois)] {
        let texte = paiements_dits(somme.nombre, somme.plus);
        l.rangees.push(Rangee::nouvelle(mot, encre, &texte).a_droite(argent(somme.montant)));
    }
    if let Some(a) = &s.abonnements {
        let texte = compte(a.actifs as f64, "actif", "actifs");
        l.rangees.push(Rangee::nouvelle("Abonnements", encre, &texte).a_droite(format!("{}/mois", argent(a.mensuel))));
    }
    let attente =
        if s.en_attente.abs() >= 0.5 { format!("{} en attente", argent(s.en_attente)) } else { String::new() };
    l.rangees.push(Rangee::nouvelle("Solde", encre, "disponible").a_droite(argent(s.disponible)).detail(&[attente]));
    if let Some(v) = &s.virement {
        let texte = format!("{} {}", if v.fait { "arrivé" } else { "arrive" }, jour_dit(&v.arrivee, m));
        l.rangees.push(Rangee::nouvelle("Virement", encre, &texte).a_droite(argent(v.montant)));
    }
    if let Some(o) = &s.ouvertes {
        let ouvertes = compte(o.nombre as f64, "ouverte", "ouvertes");
        let ouvertes = if o.plus { format!("au moins {ouvertes}") } else { ouvertes };
        let texte = match (o.nombre, o.echues) {
            (0, _) => "aucune à payer".to_string(),
            (_, 0) => ouvertes,
            (1, _) => format!("{ouvertes}, échue"),
            (n, e) if e >= n => format!("{ouvertes}, toutes échues"),
            (_, e) => format!("{ouvertes}, dont {}", compte(e as f64, "échue", "échues")),
        };
        // Des factures échues, en ambre : souvent de vieilles factures que Stripe ne relance plus (celles dont le
        // paiement d'office a échoué en sont) ; elles se lisent sans alarmer, et ne font pas monter la carte.
        let couleur = if o.echues > 0 { tete(erp::AMBRE) } else { encre };
        let mut r = Rangee::nouvelle("Factures", couleur, &texte);
        if o.nombre > 0 {
            r = r.a_droite(argent(o.du));
        }
        l.rangees.push(r);
    }
    l.pied = pied_en_ligne("Avant les frais de Stripe", s.lu_a, STRIPE_PERIMES, m);
    l
}

/// Les derniers paiements de Stripe, le plus récent d'abord : leur issue en mot de tête (réussi en vert, échoué en
/// rouge, remboursé en gris, en attente en ambre), qui a payé, le montant à droite (avec ses cents), quand en détail.
/// Dans la pilule, les échecs en rouge, sinon « Tous réussis » en vert. Un échec depuis un jour fait monter la carte en
/// haut de la page, rouge. Rien sans paiement.
fn liste_des_paiements(paiements: &[Paiement], devise: &str, m: &Maintenant) -> Option<(u8, Liste)> {
    if paiements.is_empty() {
        return None;
    }
    let echecs = paiements.iter().filter(|p| p.issue == Issue::Echoue).count();
    let recent = paiements.iter().any(|p| p.issue == Issue::Echoue && m.unix - p.quand <= ECHEC_RECENT);
    let (pilule, teinte) = if echecs > 0 {
        (compte(echecs as f64, "échoué", "échoués"), erp::ROUGE)
    } else if paiements.iter().all(|p| p.issue == Issue::Reussi) {
        ("Tous réussis".to_string(), erp::VERT)
    } else {
        (compte(paiements.len() as f64, "paiement", "paiements"), erp::INDIGO)
    };
    let carte = if recent { erp::ROUGE } else { erp::INDIGO };
    let mut l = Liste::nouvelle("Paiements Stripe", pictos::PAIEMENTS, carte).pilule(pilule, teinte);
    for p in paiements.iter().take(RANGEES) {
        let (mot, couleur) = match p.issue {
            Issue::Reussi => ("Réussi", tete(erp::VERT)),
            Issue::Echoue => ("Échoué", erp::DANGER_ENCRE),
            Issue::Rembourse => ("Remboursé", erp::TEXTE_2),
            Issue::EnAttente => ("En attente", tete(erp::AMBRE)),
        };
        let qui = Some(p.qui.trim()).filter(|q| !q.is_empty()).unwrap_or("Client");
        let quand = m.quand(p.quand);
        let quand = if quand.starts_with("à ") { format!("aujourd'hui {quand}") } else { quand };
        let montant = montant_stripe(p.montant, devise, true);
        l.rangees.push(Rangee::nouvelle(mot, couleur, qui).a_droite(montant).detail(&[quand]));
    }
    let rang = if recent { rang::EN_LIGNE_QUI_PRESSE } else { rang::STRIPE };
    Some((rang, l))
}

// ---- La page ------------------------------------------------------------------------------------------------------

/// Une carte de la maçonnerie : une liste, ou une carte d'un état de l'entreprise (l'invitation à se relier, la
/// première lecture, une lecture qui n'a pas abouti, sans chiffres d'avant).
enum Carte {
    Liste(Liste),
    Invitation,
    PremiereLecture,
    Erreur(String),
    /// Une carte épinglée (`epingles.json`, que publie le chef) : sa clé dans l'ordre de la personne, et son objet.
    Epingle {
        cle: String,
        carte: Value,
    },
}

impl Carte {
    /// Ce qui la nomme dans l'ordre de la personne : la clé de sa liste ; un état de l'entreprise prend la place de
    /// ses cartes, sous un seul nom.
    fn cle(&self) -> &str {
        match self {
            Carte::Liste(l) => &l.cle,
            Carte::Epingle { cle, .. } => cle,
            _ => "constructo-ai",
        }
    }
}

/// Une note ouverte au tableau de bord (on l'écrit) : son id, son texte tel qu'on le tape, l'heure de la dernière
/// frappe pas encore dite au chef, si elle doit prendre le clavier à la prochaine image ; refermée, l'heure où elle
/// l'a été — sa carte garde ce texte le temps que le chef le publie.
pub(super) struct NoteOuverte {
    pub(super) id: String,
    pub(super) texte: String,
    pub(super) frappe: Option<f64>,
    pub(super) clavier: bool,
    pub(super) fermee: Option<f64>,
}

/// Une note se dit au chef une demi-seconde après sa dernière frappe (et quand on la quitte) ; une carte qu'on vient
/// voir de la conversation reste soulignée deux secondes.
const PAUSE_DE_LA_NOTE: f64 = 0.6;
const MONTREE: f64 = 2.0;

/// Les lignes du menu d'une carte épinglée : une note se modifie, une carte d'Eve s'actualise ; on parle de l'une
/// comme de l'autre à Eve ; on les retire (un second clic le confirme).
const MODIFIER: &str = "Modifier";
const ACTUALISER: &str = "Actualiser";
const VOIR_LES_DONNEES: &str = "Voir les données";
const VOIR_LE_GRAPHIQUE: &str = "Voir le graphique";
const EN_PARLER: &str = "En parler à Eve";
const RETIRER: &str = "Retirer";

/// Un graphique offre aussi sa vue en tableau (`en_tableau` : elle se montre), où chaque valeur se lit sans survol.
fn lignes_du_menu(sorte: &str, en_tableau: bool) -> Vec<LigneDeMenu> {
    let mut lignes = vec![if sorte == "note" {
        LigneDeMenu { icone: pictos::MODIFIER, mot: MODIFIER, a_confirmer: false }
    } else {
        LigneDeMenu { icone: pictos::RELIRE, mot: ACTUALISER, a_confirmer: false }
    }];
    if sorte == "graphique" {
        lignes.push(if en_tableau {
            LigneDeMenu { icone: pictos::GRAPHIQUE, mot: VOIR_LE_GRAPHIQUE, a_confirmer: false }
        } else {
            LigneDeMenu { icone: crate::cartes::pictos::TABLEAU, mot: VOIR_LES_DONNEES, a_confirmer: false }
        });
    }
    lignes.push(LigneDeMenu { icone: pictos::PARLER, mot: EN_PARLER, a_confirmer: false });
    lignes.push(LigneDeMenu { icone: pictos::RETIRER, mot: RETIRER, a_confirmer: true });
    lignes
}

/// Le graphique `id` se montre-t-il en tableau (son menu : « Voir les données ») ?
fn graphique_en_tableau(ctx: &egui::Context, id: &str) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::unique(("graphique-en-tableau", id)))).unwrap_or(false)
}

/// L'icône et la teinte d'office d'une sorte de carte épinglée (la carte peut dire sa teinte).
fn allure_de_la_sorte(sorte: &str) -> (char, erp::Teinte) {
    match sorte {
        "note" => (pictos::NOTE, erp::JAUNE),
        "liste" => (pictos::LISTE, erp::BLEU),
        "chiffres" => (pictos::CALCUL, erp::VERT),
        "tableau" => (crate::cartes::pictos::TABLEAU, erp::INDIGO),
        "graphique" => (pictos::GRAPHIQUE, erp::VIOLET),
        "dessin" => (crate::cartes::pictos::DESSIN, erp::SARCELLE),
        _ => (crate::cartes::pictos::EPINGLE, erp::GRIS),
    }
}

/// Le titre d'une note : sa première ligne (sans sa puce), ou « Nouvelle note ».
fn titre_de_la_note(texte: &str) -> String {
    let premiere = texte.trim().lines().next().unwrap_or("").trim();
    let premiere = premiere.trim_start_matches(['-', '•', '*']).trim();
    if premiere.is_empty() { "Nouvelle note".into() } else { premiere.into() }
}

/// Une ligne du texte d'une note : « - », « • » ou « * » en tête, une puce et son retrait ; sinon, le texte.
fn lignes_de_note(ui: &mut egui::Ui, ligne: &str) {
    let l = ligne.trim_end();
    let puce = l.trim_start().strip_prefix(['-', '•', '*']).filter(|r| r.starts_with(' '));
    let corps = RichText::new(puce.map_or(l, str::trim)).size(14.0).color(erp::TEXTE);
    if puce.is_none() {
        if l.trim().is_empty() {
            ui.add_space(4.0);
        } else {
            ui.add(egui::Label::new(corps).wrap().selectable(false));
        }
        return;
    }
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new("•").size(14.0).color(erp::TEXTE_2));
        ui.add(egui::Label::new(corps).wrap().selectable(false));
    });
}

/// Une valeur d'une carte épinglée, en texte : une chaîne telle quelle, un nombre tel qu'écrit.
fn valeur_en_texte(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        x => x.to_string(),
    }
}

/// La liste d'une carte épinglée : ses `rangees` (un texte ; ou un objet — son mot de `tete`, dans l'encre de sa
/// `teinte`, son `texte`, sa `valeur` à droite, son `detail` gris), trente au plus.
fn liste_de_l_epingle(c: &Value, titre: &str, icone: char, teinte: erp::Teinte) -> Liste {
    let mut l = Liste::nouvelle(titre, icone, teinte);
    for r in c["rangees"].as_array().into_iter().flatten().take(30) {
        let rangee = match r {
            Value::Object(_) => {
                let encre = r["teinte"].as_str().and_then(erp::nommee).map_or(erp::TITRE, |t| t.encre);
                Rangee::nouvelle(&valeur_en_texte(&r["tete"]), encre, &valeur_en_texte(&r["texte"]))
                    .a_droite(valeur_en_texte(&r["valeur"]))
                    .detail(&[valeur_en_texte(&r["detail"])])
            }
            x => Rangee::nouvelle("", erp::TITRE, &valeur_en_texte(x)),
        };
        l.rangees.push(rangee);
    }
    l
}

/// Les chiffres d'une carte épinglée : chacun sa `valeur`, son `libelle`, son `ecart` et le sens qui est bon (`bon` :
/// « hausse » d'office, ou « baisse »), six au plus.
fn chiffres_de_l_epingle(c: &Value) -> Vec<PetitChiffre> {
    let texte = |x: &Value, cles: &[&str]| {
        cles.iter().map(|k| valeur_en_texte(&x[*k])).find(|t| !t.is_empty()).unwrap_or_default()
    };
    c["chiffres"]
        .as_array()
        .into_iter()
        .flatten()
        .take(6)
        .map(|x| PetitChiffre {
            valeur: texte(x, &["valeur"]),
            etiquette: texte(x, &["libelle", "etiquette"]),
            ecart: texte(x, &["ecart", "tendance"]),
            baisse_bonne: eve::parole::normaliser(x["bon"].as_str().unwrap_or("")) == "baisse",
        })
        .collect()
}

/// La ligne grise sous une carte épinglée : sa `mention` (« Source : Constructo AI ») ; sans elle, pour une carte
/// d'Eve, quand elle l'a épinglée ou mise à jour.
fn mention_de_l_epingle(c: &Value) -> String {
    let mention = valeur_en_texte(&c["mention"]);
    if !mention.trim().is_empty() || c["de"] == "personne" || c["sorte"] == "note" {
        return mention;
    }
    let a = c["mis_a_jour"].as_str().unwrap_or("");
    match crate::rappels::heure_ecrite(a.get(..16).unwrap_or(a)) {
        Some(a) => format!("Mise à jour par Eve {}", quand_de_la_mise_a_jour(&a)),
        None => "Épinglée par Eve".into(),
    }
}

/// Quand une carte d'Eve a été mise à jour, comme on le dit : « à 9 h 12 », « demain à 9 h », « le 27 octobre ».
fn quand_de_la_mise_a_jour(a: &str) -> String {
    let jour = crate::journal::date();
    match crate::rappels::heure_dite(a, &jour) {
        h if h.starts_with(|c: char| c.is_ascii_digit()) => format!("à {h}"),
        h => h,
    }
}

/// L'ordre des cartes qu'on vient de donner d'un geste, tenu le temps que le chef le publie.
const ORDRE_TENU: &str = "ordre-du-tableau-donne-d-un-geste";

impl Fenetre {
    /// La vue du tableau de bord : celle que publie le chef, ou celle qu'on vient de choisir d'un clic, tenue le temps
    /// qu'il la publie (pas de retour en arrière d'une image).
    fn vue_du_tableau(&self, ctx: &egui::Context) -> Vue {
        let maintenant = ctx.input(|i| i.time);
        match ctx.data(|d| d.get_temp::<(usize, f64)>(egui::Id::unique(VUE_TENUE))) {
            Some((k, quand)) if maintenant - quand < TENUE => VUES[k.min(VUES.len() - 1)].0,
            _ => Vue::de(self.champ("vue_du_tableau")),
        }
    }

    /// L'ordre des cartes que la personne a donné en les déplaçant : celui que publie le chef, ou celui qu'on vient de
    /// donner d'un geste, tenu le temps qu'il le publie.
    fn ordre_du_tableau(&self, ctx: &egui::Context) -> Vec<String> {
        let maintenant = ctx.input(|i| i.time);
        match ctx.data(|d| d.get_temp::<(Vec<String>, f64)>(egui::Id::unique(ORDRE_TENU))) {
            Some((ordre, quand)) if maintenant - quand < TENUE => ordre,
            _ => self.etat["ordre_du_tableau"]
                .as_array()
                .map(|l| l.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default(),
        }
    }

    /// Ce qu'on a choisi dans le menu d'une rangée (`k`) d'une carte : une demande à Eve, envoyée comme si on l'avait
    /// écrite (elle s'affiche dans la conversation) — une relance, un suivi, « c'est fait », en parler —, ou un rappel
    /// pour demain matin, que le chef pose sans Claude. Une relance, un suivi : un brouillon, jamais envoyé.
    fn agir(&self, l: &Liste, k: usize, action: Action) {
        let Some(r) = l.rangees.get(k) else { return };
        let (titre, quoi) = (l.titre.as_str(), theme::texte_de_rangee(r));
        let demande = match action {
            Action::Relance => format!(
                "Prépare une relance courtoise pour cette facture en retard (« {titre} », au tableau de bord) : {quoi}. \
                 Fais-en un brouillon, ne l'envoie pas."
            ),
            Action::Suivi => format!(
                "Prépare un suivi pour cette soumission (« {titre} », au tableau de bord) : {quoi}. Fais-en un \
                 brouillon, ne l'envoie pas."
            ),
            Action::Fait => format!(
                "C'est fait (« {titre} », au tableau de bord) : {quoi}. Mets le tableau de bord à jour : retire-le, \
                 ou passe-le à sa prochaine date s'il revient."
            ),
            Action::Demander => format!("Parlons de ceci, au tableau de bord (« {titre} ») : {quoi}"),
            Action::RappelDemain => {
                self.geste("rappel_demain", Value::String(format!("{titre} : {quoi}")));
                return;
            }
        };
        self.geste("texte", Value::String(demande));
    }

    /// Un ordre donné d'un geste : tenu ici, et dit au chef, qui l'inscrit dans `reglages.json`.
    fn donner_l_ordre(&self, ctx: &egui::Context, ordre: Vec<String>) {
        let maintenant = ctx.input(|i| i.time);
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(ORDRE_TENU), (ordre.clone(), maintenant)));
        self.geste("ordre_du_tableau", Value::from(ordre));
    }

    /// Le tableau de bord sous sa bannière : la rangée du choix et de la fraîcheur ; le bandeau d'une lecture manquée
    /// ou d'une liaison perdue ; les tuiles des chiffres ; les cartes en maçonnerie.
    pub(super) fn page_du_tableau(&mut self, ui: &mut egui::Ui, aujourdhui: &str, heure: &str) {
        let lecture = self.centre.entreprise();
        let liaison = Liaison::de(&self.etat["constructo"], lecture.as_ref());
        let lecture = if liaison == Liaison::Absente { None } else { lecture };
        let m = Maintenant::du_poste();
        let vue = if liaison == Liaison::Absente { Vue::Famille } else { self.vue_du_tableau(ui.ctx()) };
        let (entreprise, famille) = (vue != Vue::Famille, vue != Vue::Entreprise);
        let ordre_des_cartes = self.ordre_du_tableau(ui.ctx());
        let f = (entreprise && liaison != Liaison::Absente).then(|| fraicheur(liaison, lecture.as_ref(), &m));
        self.rangee_du_choix(ui, vue, f.as_ref(), liaison, !ordre_des_cartes.is_empty());
        ui.add_space(14.0);
        let lue = lecture.as_ref().and_then(|l| l.as_ref().ok()).filter(|v| a_des_chiffres(v)).cloned();
        // Le bandeau : une lecture manquée (les chiffres d'avant restent), ou une liaison perdue (ils pâlissent).
        let mut pales = false;
        if entreprise && let Some(v) = &lue {
            let quand =
                lue_quand(v, &m).map_or(String::new(), |(_, q)| format!(" Ces chiffres datent {}.", de_quand(&q)));
            if liaison != Liaison::Reliee {
                pales = true;
                let texte = format!("Constructo AI n'est plus relié.{quand}");
                if bandeau(ui, false, &texte, ("Se reconnecter", pictos::LIEN)) {
                    self.geste("constructo_connecter", Value::Null);
                }
                ui.add_space(ECART);
            } else if texte_de(v, "etat") == "erreur" {
                let raison = texte_de(v, "erreur");
                let texte = if raison.is_empty() {
                    format!("La dernière lecture de Constructo AI n'a pas abouti.{quand}")
                } else {
                    format!("{}{quand}", phrase(raison))
                };
                if bandeau(ui, true, &texte, ("Réessayer", pictos::RELIRE)) {
                    self.geste("entreprise_lire", Value::Null);
                }
                ui.add_space(ECART);
            }
        }
        // Les tuiles : les chiffres de la vue, dans son ordre, six au plus.
        let mut chiffres = if famille { self.centre.chiffres(aujourdhui, heure) } else { Vec::new() };
        if entreprise && let Some(v) = &lue {
            chiffres.extend(chiffres_de_l_entreprise(v));
        }
        let ordre: &[&str] = match vue {
            Vue::Tout => &TUILES_DE_TOUT,
            Vue::Entreprise => &TUILES_DE_L_ENTREPRISE,
            Vue::Famille => &TUILES_DE_LA_FAMILLE,
        };
        let tuiles: Vec<Chiffre> = ordre
            .iter()
            .filter_map(|cle| chiffres.iter().find(|(c, _)| c == cle).map(|(_, x)| x.clone()))
            .take(SIX)
            .collect();
        if !tuiles.is_empty() {
            ui.scope(|ui| {
                if pales && vue == Vue::Entreprise {
                    ui.multiply_opacity(0.55);
                }
                theme::tuiles(ui, &tuiles);
            });
            ui.add_space(ECART);
        }
        // Les cartes : celles de la maison et de l'entreprise, rangées ensemble ; un état de l'entreprise sans chiffres
        // y prend sa place, en tête.
        let mut cartes: Vec<(u8, bool, Carte)> = Vec::new();
        if famille {
            cartes.extend(self.centre.listes(aujourdhui, heure).into_iter().map(|(r, l)| (r, false, Carte::Liste(l))));
        }
        // La météo vaut pour la maison comme pour les chantiers : dans toutes les vues.
        if let Some(b) = &self.bulletin {
            let ville = self.meteo.as_ref().map(|x| x.lieu().ville.clone()).unwrap_or_default();
            if let Some(l) = liste_de_la_meteo(b, &ville, &m) {
                cartes.push((rang::METEO, false, Carte::Liste(l)));
            }
        }
        if entreprise {
            match (&lue, liaison) {
                (Some(v), _) => {
                    let listes = listes_de_l_entreprise(v, &m).into_iter();
                    cartes.extend(listes.map(|(r, l)| (r, pales, Carte::Liste(l))));
                }
                (None, Liaison::NonReliee | Liaison::EnCours) => {
                    cartes.push((rang::ALERTES_HAUTES, false, Carte::Invitation));
                }
                (None, _) => {
                    let erreur = match &lecture {
                        Some(Err(raison)) => Some(format!("Sa dernière lecture (entreprise.json) {raison}.")),
                        Some(Ok(v)) if texte_de(v, "etat") == "erreur" => match texte_de(v, "erreur") {
                            "" => Some("La lecture n'a pas abouti.".to_string()),
                            raison => Some(phrase(raison)),
                        },
                        _ => None,
                    };
                    cartes.push((rang::ALERTES_HAUTES, false, erreur.map_or(Carte::PremiereLecture, Carte::Erreur)));
                }
            }
        }
        // En ligne (les services, les erreurs de Render, Stripe) : avec l'entreprise, ou avec la maison seule quand
        // Constructo AI n'est pas permis ; jamais pâlies, elles ne viennent pas de l'ERP.
        if entreprise || liaison == Liaison::Absente {
            let en_ligne = listes_en_ligne(self.releve.as_ref(), self.montants.as_ref(), &m);
            cartes.extend(en_ligne.into_iter().map(|(r, l)| (r, false, Carte::Liste(l))));
        }
        // Ce qu'on a épinglé, dans la vue qu'il dit.
        cartes.extend(self.epingles_de_la_vue(entreprise, famille).into_iter().map(|c| (rang::EPINGLES, false, c)));
        cartes.sort_by_key(|(r, _, _)| *r);
        theme::ranger(&mut cartes, |(_, _, c)| c.cle(), &ordre_des_cartes);
        let cles: Vec<String> = cartes.iter().map(|(_, _, c)| c.cle().to_string()).collect();
        let colonnes = theme::colonnes_pour(ui.available_width());
        let deplacement = theme::maconnerie(ui, colonnes, &cles, |ui, k| {
            let (_, pale, carte) = &cartes[k];
            if *pale {
                ui.multiply_opacity(0.55);
            }
            match carte {
                // Une rangée qui ouvre une page (un service et sa page d'état) : le chef l'ouvre, `https://` seulement.
                Carte::Liste(l) => match theme::carte_liste(ui, l) {
                    Some(Clic::Page(page)) => self.geste("ouvrir_lien", Value::String(page)),
                    Some(Clic::Action(a, k)) => self.agir(l, k, a),
                    None => {}
                },
                Carte::Invitation => self.invitation(ui, liaison),
                Carte::PremiereLecture => premiere_lecture(ui),
                Carte::Erreur(raison) => self.carte_d_erreur(ui, raison),
                Carte::Epingle { carte, .. } => self.carte_epinglee(ui, carte),
            }
        });
        // Une carte lâchée ailleurs : l'ordre de ce qui se voit, puis celui des cartes d'une autre vue (ou sans
        // données aujourd'hui), qui gardent le leur.
        if let Some(d) = deplacement {
            let mut neuf = cles.clone();
            let carte = neuf.remove(d.de);
            neuf.insert(d.vers.min(neuf.len()), carte);
            neuf.extend(ordre_des_cartes.into_iter().filter(|c| !cles.contains(c)));
            self.donner_l_ordre(ui.ctx(), neuf);
        }
    }

    /// La rangée au-dessus de tout (le filtre des tableaux de bord : un seul, au-dessus de ce qu'il touche) : le choix
    /// de la vue en segments ; à droite, quand l'entreprise se montre, l'heure de sa dernière lecture et le bouton qui
    /// la relit (reliée seulement ; une roue pendant qu'Eve lit).
    fn rangee_du_choix(
        &mut self,
        ui: &mut egui::Ui,
        vue: Vue,
        f: Option<&Fraicheur>,
        liaison: Liaison,
        ordre_donne: bool,
    ) {
        let largeur = ui.available_width();
        let (rangee, _) = ui.allocate_exact_size(vec2(largeur, 38.0), Sense::hover());
        let mut gauche = ui.new_child(UiBuilder::new().max_rect(rangee).layout(Layout::left_to_right(Align::Center)));
        if liaison != Liaison::Absente
            && let Some(choisie) = segments_du_tableau(&mut gauche, vue)
            && choisie != vue
        {
            let maintenant = ui.input(|i| i.time);
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(VUE_TENUE), (choisie.rang(), maintenant)));
            self.geste("vue_du_tableau", Value::String(VUES_DU_TABLEAU[choisie.rang()].0.into()));
        }
        // De quoi épingler une carte : une note, tout de suite ; ou une demande à Eve, qui épingle n'importe quoi.
        if liaison != Liaison::Absente {
            gauche.add_space(10.0);
        }
        self.bouton_ajouter(&mut gauche);
        // Les cartes déplacées : de quoi revenir à l'ordre d'office (ce qui presse en tête).
        if ordre_donne {
            gauche.add_space(10.0);
            let rep = theme::bouton_erp(&mut gauche, Some(pictos::RELIRE), "Ordre d'office", BoutonErp::Fantome);
            if rep.on_hover_text("Les cartes reprennent leur place d'office : ce qui presse en tête").clicked() {
                self.donner_l_ordre(ui.ctx(), Vec::new());
            }
        }
        let Some(f) = f else { return };
        let reste = (rangee.right() - gauche.min_rect().right() - 16.0).max(0.0);
        let mut droite = ui.new_child(UiBuilder::new().max_rect(rangee).layout(Layout::right_to_left(Align::Center)));
        let lit = f.point == Point::Roue && liaison == Liaison::Reliee;
        let rep = bouton_relire(&mut droite, liaison == Liaison::Reliee, lit);
        if rep.clicked() && liaison == Liaison::Reliee && !lit {
            self.geste("entreprise_lire", Value::Null);
        }
        droite.add_space(10.0);
        mise_a_jour_dite(&mut droite, f, reste - 50.0);
    }

    /// Les cartes épinglées de la vue : celles que publie le chef (`epingles`, la plus récente en tête), dans la vue
    /// qu'elles disent (sans vue, partout) ; la note qu'on écrit par-dessus la sienne, ou en tête avant que le chef la
    /// publie ; une note qu'on vient de vider ne se montre plus.
    fn epingles_de_la_vue(&self, entreprise: bool, famille: bool) -> Vec<Carte> {
        let mut liste: Vec<Value> = self.etat["epingles"].as_array().cloned().unwrap_or_default();
        if let Some(n) = &self.note {
            match liste.iter_mut().find(|c| c["id"] == n.id.as_str()) {
                Some(c) => c["texte"] = Value::String(n.texte.clone()),
                None => liste
                    .insert(0, serde_json::json!({ "id": n.id, "sorte": "note", "de": "personne", "texte": n.texte })),
            }
            if n.fermee.is_some() && n.texte.trim().is_empty() {
                liste.retain(|c| c["id"] != n.id.as_str());
            }
        }
        liste
            .into_iter()
            .filter(|c| match eve::parole::normaliser(c["vue"].as_str().unwrap_or("")).as_str() {
                "entreprise" => entreprise,
                "famille" => famille,
                _ => true,
            })
            .map(|c| Carte::Epingle { cle: format!("epingle-{}", c["id"].as_str().unwrap_or("")), carte: c })
            .collect()
    }

    /// Une carte épinglée : sa tête (la pastille de sa sorte, son titre, le menu « ⋯ »), ce que sa sorte montre — une
    /// note, une liste, des chiffres, un tableau, un dessin —, sa mention grise. Une note s'écrit sur place : un clic
    /// sur elle l'ouvre. Venue de la conversation (« Voir au tableau de bord »), elle défile sous les yeux, soulignée
    /// un moment.
    fn carte_epinglee(&mut self, ui: &mut egui::Ui, c: &Value) {
        let id = c["id"].as_str().unwrap_or("").to_string();
        let sorte = c["sorte"].as_str().unwrap_or("note");
        let (icone, teinte) = allure_de_la_sorte(sorte);
        let teinte = c["teinte"].as_str().and_then(erp::nommee).unwrap_or(teinte);
        let texte = c["texte"].as_str().unwrap_or("").to_string();
        let ouverte = self.note.as_ref().is_some_and(|n| n.id == id && n.fermee.is_none());
        let titre = match sorte {
            "note" => titre_de_la_note(&texte),
            _ => c["titre"].as_str().filter(|t| !t.trim().is_empty()).unwrap_or("Carte").to_string(),
        };
        let en_tableau = graphique_en_tableau(ui.ctx(), &id);
        let lignes = lignes_du_menu(sorte, en_tableau);
        let mut choisie = None;
        let mut clic = None;
        theme::carte(ui, Some(teinte.pastel), 0.0, |ui| {
            let (ligne, tete) = theme::tete_avec_menu(ui, &titre, icone, teinte, &id, &lignes);
            choisie = ligne;
            match sorte {
                "note" => self.corps_de_note(ui, &id, &texte, ouverte, tete),
                "liste" => clic = theme::corps_de_liste(ui, &liste_de_l_epingle(c, &titre, icone, teinte)),
                "chiffres" => theme::chiffres_de_carte(ui, &chiffres_de_l_epingle(c)),
                "tableau" => self.cartes.tableau_seul(ui, &c.to_string()),
                // Un graphique, ou sa vue en tableau (son menu passe de l'un à l'autre).
                "graphique" => match graphiques::Graphique::de(c) {
                    Ok(g) if en_tableau => self.cartes.tableau_seul(ui, &g.en_tableau()),
                    Ok(g) => graphiques::graphique(ui, &g, &id),
                    Err(raison) => {
                        ui.add(egui::Label::new(RichText::new(raison).size(13.0).color(erp::TEXTE_2)).wrap());
                    }
                },
                "dessin" => {
                    if let Some(g) = self.cartes.dessin_seul(ui, c["svg"].as_str().unwrap_or("")) {
                        self.geste(g.action, g.valeur);
                    }
                }
                _ => {
                    let quoi = format!("Eve ne sait pas encore dessiner une carte « {sorte} ».");
                    ui.add(egui::Label::new(RichText::new(quoi).size(13.0).color(erp::TEXTE_2)).wrap());
                }
            }
            let mention = mention_de_l_epingle(c);
            if !mention.is_empty() {
                ui.add_space(2.0);
                ui.add(
                    egui::Label::new(RichText::new(mention).size(12.0).color(erp::TEXTE_3)).wrap().selectable(false),
                );
            }
        });
        let rect = ui.min_rect();
        let liste = || liste_de_l_epingle(c, &titre, icone, teinte);
        match clic {
            Some(Clic::Page(page)) => self.geste("ouvrir_lien", Value::String(page)),
            Some(Clic::Action(a, k)) => self.agir(&liste(), k, a),
            None => {}
        }
        if let Some(k) = choisie {
            self.agir_sur_l_epingle(ui.ctx(), c, &titre, lignes[k].mot);
        }
        self.montrer_si_demandee(ui, &id, rect);
    }

    /// Le texte d'une note sous son titre (sa première ligne) : fermée, ses autres lignes (« - » en tête : une puce), et
    /// un clic sur elle ou sur son titre (`tete`) l'ouvre ; ouverte, le champ où on l'écrit tout entière, qui prend le
    /// clavier — Échap ou un clic ailleurs la referme.
    fn corps_de_note(&mut self, ui: &mut egui::Ui, id: &str, texte: &str, ouverte: bool, tete: Rect) {
        let maintenant = ui.input(|i| i.time);
        if ouverte && let Some(n) = self.note.as_mut() {
            let champ = egui::TextEdit::multiline(&mut n.texte)
                .id(egui::Id::unique(("note-du-tableau", id)))
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .font(FontId::proportional(14.0))
                .text_color(erp::TEXTE)
                .hint_text("Écris ta note… Sa première ligne est son titre.")
                .frame(
                    egui::Frame::new()
                        .fill(Color32::WHITE)
                        .stroke((1.0, erp::PRIMAIRE))
                        .corner_radius(8)
                        .inner_margin(egui::Margin::symmetric(10, 8)),
                );
            let rep = ui.add(champ);
            if std::mem::take(&mut n.clavier) {
                rep.request_focus();
            }
            if rep.changed() {
                n.frappe = Some(maintenant);
            }
            if rep.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                rep.surrender_focus();
            }
            if rep.lost_focus() {
                self.fermer_la_note(maintenant);
            }
            let aide = "Elle s'enregistre seule. Échap ou un clic ailleurs : c'est fini.";
            ui.add(egui::Label::new(RichText::new(aide).size(11.5).color(erp::TEXTE_3)).wrap().selectable(false));
            return;
        }
        let debut = ui.cursor().min;
        let corps: Vec<&str> = texte.trim().lines().skip(1).collect();
        let corps: Vec<&str> = corps.iter().copied().skip_while(|l| l.trim().is_empty()).collect();
        if texte.trim().is_empty() {
            let vide = "Une note vide : clique pour l'écrire.";
            ui.add(egui::Label::new(RichText::new(vide).size(13.5).color(erp::TEXTE_3)).wrap().selectable(false));
        }
        for ligne in &corps {
            lignes_de_note(ui, ligne);
        }
        let corps = Rect::from_min_max(debut, pos2(debut.x + ui.available_width(), ui.min_rect().bottom()));
        let mut ouvrir = false;
        for (k, place) in [tete, corps].into_iter().enumerate().filter(|(_, r)| r.height() > 0.0) {
            let rep = ui.interact(place, egui::Id::unique(("ouvrir-la-note", id, k)), Sense::click());
            ouvrir |= rep.on_hover_cursor(egui::CursorIcon::Text).on_hover_text("Modifier la note").clicked();
        }
        if ouvrir {
            self.ouvrir_la_note(id, texte, maintenant);
        }
    }

    /// Ouvre la note `id` (son texte) : la note déjà ouverte se referme d'abord.
    pub(super) fn ouvrir_la_note(&mut self, id: &str, texte: &str, maintenant: f64) {
        if self.note.as_ref().is_some_and(|n| n.id == id && n.fermee.is_none()) {
            return;
        }
        self.fermer_la_note(maintenant);
        self.note = Some(NoteOuverte { id: id.into(), texte: texte.into(), frappe: None, clavier: true, fermee: None });
    }

    /// Une note neuve, en tête du tableau de bord, qui prend le clavier ; rien ne s'écrit tant qu'elle est vide.
    fn ajouter_une_note(&mut self, maintenant: f64) {
        let millis = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        self.ouvrir_la_note(&format!("note-{millis}"), "", maintenant);
    }

    /// La note qu'on quitte : ce que le chef n'en sait pas encore part (vide, elle s'en va du tableau de bord) ; sa
    /// carte garde son texte le temps qu'il le publie.
    pub(super) fn fermer_la_note(&mut self, maintenant: f64) {
        let Some(n) = self.note.as_mut().filter(|n| n.fermee.is_none()) else { return };
        n.fermee = Some(maintenant);
        let (id, texte, frappe) = (n.id.clone(), n.texte.clone(), n.frappe.take());
        let publiee = self.etat["epingles"].as_array().is_some_and(|l| l.iter().any(|c| c["id"] == id.as_str()));
        if texte.trim().is_empty() {
            if publiee {
                self.geste("epingle_retirer", Value::String(id));
            }
        } else if frappe.is_some() || !publiee {
            self.geste("epingle_note", serde_json::json!({ "id": id, "texte": texte }));
        }
    }

    /// À chaque image : la note ouverte se dit au chef une demi-seconde après sa dernière frappe ; une note refermée
    /// s'oublie dès que le chef publie son texte (ou qu'il l'a retirée), au plus tard après le temps de tenue.
    pub(super) fn suivre_la_note(&mut self, maintenant: f64) {
        let Some(n) = self.note.as_mut() else { return };
        if n.fermee.is_none() && n.frappe.is_some_and(|f| maintenant - f >= PAUSE_DE_LA_NOTE) {
            n.frappe = None;
            let (id, texte) = (n.id.clone(), n.texte.clone());
            if !texte.trim().is_empty() {
                self.geste("epingle_note", serde_json::json!({ "id": id, "texte": texte }));
            }
            return;
        }
        let Some(fermee) = n.fermee else { return };
        let publiee = self.etat["epingles"].as_array().and_then(|l| l.iter().find(|c| c["id"] == n.id.as_str()));
        let rendue = match publiee {
            Some(c) => c["texte"].as_str() == Some(n.texte.as_str()),
            None => n.texte.trim().is_empty(),
        };
        if rendue || maintenant - fermee > TENUE {
            self.note = None;
        }
    }

    /// Ce qu'on a choisi dans le menu d'une carte épinglée (`mot`, sa ligne) : la modifier (une note), l'actualiser ou
    /// en parler (une demande à Eve, comme si on l'avait écrite), la retirer.
    fn agir_sur_l_epingle(&mut self, ctx: &egui::Context, c: &Value, titre: &str, mot: &str) {
        let maintenant = ctx.input(|i| i.time);
        let id = c["id"].as_str().unwrap_or("").to_string();
        let texte = c["texte"].as_str().unwrap_or("");
        match mot {
            MODIFIER => self.ouvrir_la_note(&id, texte, maintenant),
            VOIR_LES_DONNEES | VOIR_LE_GRAPHIQUE => {
                let cle = egui::Id::unique(("graphique-en-tableau", id.as_str()));
                ctx.data_mut(|d| d.insert_temp(cle, mot == VOIR_LES_DONNEES));
            }
            ACTUALISER => {
                let source = c["source"].as_str().filter(|s| !s.trim().is_empty()).map(|s| format!(" ({s})"));
                let demande = format!(
                    "Actualise la carte « {titre} » du tableau de bord (id {id}) : refais-la d'après sa source{}, puis \
                     renvoie-la avec le même id.",
                    source.unwrap_or_default()
                );
                self.geste("texte", Value::String(demande));
            }
            EN_PARLER if c["sorte"] == "note" => {
                self.geste(
                    "texte",
                    Value::String(format!("Parlons de ma note du tableau de bord (id {id}) : {texte}")),
                );
            }
            EN_PARLER => {
                self.geste(
                    "texte",
                    Value::String(format!("Parlons de la carte « {titre} » du tableau de bord (id {id}).")),
                );
            }
            _ => {
                if self.note.as_ref().is_some_and(|n| n.id == id) {
                    self.note = None;
                }
                self.geste("epingle_retirer", Value::String(id));
            }
        }
    }

    /// La carte qu'on vient voir de la conversation (`voir_epingle`) : à sa première image, elle défile au milieu de la
    /// vue ; un anneau de l'accent la souligne, puis s'efface.
    fn montrer_si_demandee(&mut self, ui: &mut egui::Ui, id: &str, rect: Rect) {
        let maintenant = ui.input(|i| i.time);
        let Some((voulue, depuis, defile)) = self.epingle_a_montrer.as_mut() else { return };
        if voulue != id {
            return;
        }
        if !*defile {
            *defile = true;
            *depuis = maintenant;
            ui.scroll_to_rect(rect, Some(Align::Center));
        }
        let age = maintenant - *depuis;
        if age > MONTREE {
            self.epingle_a_montrer = None;
            return;
        }
        let alpha = (1.0 - (age / MONTREE) as f32).clamp(0.0, 1.0);
        let anneau = Stroke::new(2.5, erp::PRIMAIRE.gamma_multiply(alpha));
        ui.painter().rect_stroke(rect.expand(3.0), 17.0, anneau, StrokeKind::Outside);
        ui.ctx().request_repaint();
    }

    /// « Ajouter », dans la rangée du haut : son menu offre une note (tout de suite, qui prend le clavier) ou une
    /// demande à Eve — la saisie commence la phrase, et Eve épingle ce qu'on lui demande.
    fn bouton_ajouter(&mut self, ui: &mut egui::Ui) {
        let id = Some(egui::Id::unique("ajouter-une-carte"));
        let rep = theme::bouton_erp_nomme(ui, id, Some(pictos::AJOUTER), "Ajouter", BoutonErp::Secondaire);
        let rep = rep.on_hover_text("Ajouter une carte au tableau de bord : une note, ou ce que tu demandes à Eve");
        let lignes = [
            LigneDeMenu { icone: pictos::NOTE, mot: "Une note", a_confirmer: false },
            LigneDeMenu { icone: pictos::PARLER, mot: "Demander une carte à Eve…", a_confirmer: false },
        ];
        match theme::menu_du_bouton(&rep, "ajouter-une-carte", &lignes, false) {
            Some(0) => self.ajouter_une_note(ui.input(|i| i.time)),
            Some(_) => {
                self.saisie = "Épingle au tableau de bord ".into();
                self.saisie_au_clavier = true;
            }
            None => {}
        }
    }

    /// L'invitation à relier Eve à Constructo AI : ce que le tableau montrera, et le bouton qui ouvre la page de
    /// connexion (le geste `constructo_connecter`) ; la page déjà ouverte, une roue, la ligne du chef, et de quoi la
    /// rouvrir.
    fn invitation(&mut self, ui: &mut egui::Ui, liaison: Liaison) {
        let detail = self.etat["constructo"]["detail"].as_str().unwrap_or("").to_string();
        let mut clic = false;
        theme::carte(ui, Some(erp::BLEU.pastel), 0.0, |ui| {
            theme::tete_de_carte(ui, "Relie Eve à Constructo AI", pictos::LIEN, erp::BLEU, false, None);
            let promesse = "Tes projets, ce qui est à recevoir et en retard, tes devis, tes ventes et ta banque : Eve \
                            les lit dans ton ERP, sans jamais y écrire, et les tient à jour ici.";
            ui.add(egui::Label::new(RichText::new(promesse).size(14.0).color(erp::TEXTE)).wrap().selectable(false));
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (icone, mot, teinte) in [
                    (pictos::PROJETS, "Projets", erp::BLEU),
                    (pictos::RETARD, "Factures", erp::ROUGE),
                    (pictos::DEVIS, "Devis", erp::INDIGO),
                    (pictos::PIPELINE, "Ventes", erp::VIOLET),
                    (pictos::ENCAISSE, "Banque", erp::SARCELLE),
                ] {
                    puce(ui, icone, mot, teinte);
                }
            });
            ui.add_space(8.0);
            if liaison == Liaison::EnCours {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                    roue(ui, r.center(), 7.0, erp::PRIMAIRE);
                    let texte = if detail.is_empty() { "Connexion en cours…" } else { detail.as_str() };
                    ui.add(egui::Label::new(RichText::new(texte).size(13.0).color(erp::TEXTE_2)).wrap());
                });
                ui.add_space(6.0);
                clic = theme::bouton_erp(ui, Some(pictos::LIEN), "Rouvrir la page", BoutonErp::Secondaire).clicked();
            } else {
                clic = theme::bouton_erp(ui, Some(pictos::LIEN), "Se connecter à Constructo AI", BoutonErp::Primaire)
                    .on_hover_text("La page de connexion de Constructo AI s'ouvre dans ton navigateur")
                    .clicked();
            }
        });
        if clic {
            self.geste("constructo_connecter", Value::Null);
        }
    }

    /// Une lecture qui n'a pas abouti, sans chiffres d'avant : pourquoi, et « Réessayer » (le geste `entreprise_lire`).
    fn carte_d_erreur(&mut self, ui: &mut egui::Ui, raison: &str) {
        let mut clic = false;
        theme::carte(ui, Some(erp::ROUGE.pastel), 0.0, |ui| {
            theme::tete_de_carte(ui, "Constructo AI ne répond pas", pictos::ALERTES, erp::ROUGE, true, None);
            ui.add(egui::Label::new(RichText::new(raison).size(14.0).color(erp::TEXTE)).wrap().selectable(false));
            let aide = "Eve réessaiera d'elle-même ; tu peux aussi relancer la lecture.";
            ui.add(egui::Label::new(RichText::new(aide).size(12.5).color(erp::TEXTE_2)).wrap().selectable(false));
            ui.add_space(6.0);
            clic = theme::bouton_erp(ui, Some(pictos::RELIRE), "Réessayer", BoutonErp::Secondaire).clicked();
        });
        if clic {
            self.geste("entreprise_lire", Value::Null);
        }
    }
}

/// La première lecture de l'entreprise, pas encore là : une roue, et ce qu'Eve fait (le serveur de Constructo AI,
/// endormi, se réveille en une trentaine de secondes).
fn premiere_lecture(ui: &mut egui::Ui) {
    theme::carte(ui, Some(erp::BLEU.pastel), 0.0, |ui| {
        theme::tete_de_carte(ui, "Entreprise", pictos::ENTREPRISE, erp::BLEU, false, None);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            roue(ui, r.center(), 7.0, erp::PRIMAIRE);
            ui.label(RichText::new("Eve lit Constructo AI…").size(14.0).color(erp::TEXTE));
        });
        let aide = "S'il dormait, le serveur se réveille : une trentaine de secondes.";
        ui.add(egui::Label::new(RichText::new(aide).size(12.5).color(erp::TEXTE_2)).wrap().selectable(false));
    });
}

/// De quand datent des chiffres : « à 15 h 40 » → « de 15 h 40 », « hier à 18 h » → « d'hier à 18 h », « le 24 oct. à
/// 9 h » → « du 24 oct. à 9 h ».
fn de_quand(q: &str) -> String {
    if let Some(reste) = q.strip_prefix("le ") {
        format!("du {reste}")
    } else if let Some(reste) = q.strip_prefix("à ") {
        format!("de {reste}")
    } else {
        format!("d'{q}")
    }
}

/// Une raison en phrase : la première lettre en capitale, un point au bout s'il n'a pas sa ponctuation.
fn phrase(t: &str) -> String {
    let p = capitale(t);
    if p.is_empty() || p.ends_with(['.', '!', '?', '…']) { p } else { format!("{p}.") }
}

/// Un texte qui commence par une capitale : « mis à jour à 9 h » → « Mis à jour à 9 h ».
fn capitale(t: &str) -> String {
    let mut c = t.trim().chars();
    c.next().map_or_else(String::new, |p| p.to_uppercase().collect::<String>() + c.as_str())
}

/// Le choix de la vue, en segments sur une piste blanche en pilule : chacun son icône et son nom ; celui qui est choisi
/// en pilule marine (le dégradé de la bannière), son texte blanc ; un survol, un lavis gris. Rend la vue cliquée.
fn segments_du_tableau(ui: &mut egui::Ui, choisie: Vue) -> Option<Vue> {
    let police = FontId::new(13.5, FontFamily::Name(SEMI.into()));
    let icones = FontId::new(13.0, FontFamily::Name(ICONES.into()));
    let p = ui.painter().clone();
    let aucune = Color32::PLACEHOLDER;
    let mises: Vec<_> = VUES
        .iter()
        .zip(VUES_DU_TABLEAU)
        .map(|((_, icone), (_, mot))| {
            let g_icone = p.layout_no_wrap(icone.to_string(), icones.clone(), aucune);
            let g_mot = p.layout_no_wrap(mot.to_string(), police.clone(), aucune);
            let l = 16.0 + g_icone.size().x + 8.0 + g_mot.size().x + 18.0;
            (g_icone, g_mot, l)
        })
        .collect();
    let (marge, ecart) = (4.0, 2.0);
    let total = mises.iter().map(|m| m.2).sum::<f32>() + 2.0 * marge + ecart * (mises.len() - 1) as f32;
    let (piste, _) = ui.allocate_exact_size(vec2(total, 38.0), Sense::hover());
    p.add(theme::ombre_marine(piste, 19, 2, 8, 16));
    p.rect(piste, 19.0, Color32::from_white_alpha(225), Stroke::new(1.0, erp::FILET_DE_CARTE), StrokeKind::Inside);
    let mut x = piste.left() + marge;
    let mut clic = None;
    for (k, (g_icone, g_mot, l)) in mises.into_iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, piste.top() + marge), vec2(l, piste.height() - 2.0 * marge));
        x += l + ecart;
        let vue = VUES[k].0;
        let rep = ui.interact(r, egui::Id::unique(("vue-du-tableau", k)), Sense::click());
        let choisi = vue == choisie;
        if choisi {
            p.add(theme::ombre_marine(r, 15, 2, 6, 40));
            let arrets = [erp::MARINE_HAUT, erp::AZUR];
            p.add(theme::rectangle_degrade(r, r.height() / 2.0, |q| theme::degrade_css(r, 100.0, &arrets, q)));
        } else if rep.hovered() {
            p.rect_filled(r, r.height() / 2.0, erp::SURVOL);
        }
        let encre = match (choisi, rep.hovered()) {
            (true, _) => Color32::WHITE,
            (false, true) => erp::TEXTE,
            _ => erp::TEXTE_2,
        };
        let l_icone = g_icone.size().x;
        p.galley(pos2(r.left() + 16.0, r.center().y - g_icone.size().y / 2.0), g_icone, encre);
        p.galley(pos2(r.left() + 16.0 + l_icone + 8.0, r.center().y - g_mot.size().y / 2.0), g_mot, encre);
        let aide = match vue {
            Vue::Tout => "L'entreprise et la famille ensemble",
            Vue::Entreprise => "L'entreprise : Constructo AI",
            Vue::Famille => "La famille et la maison",
        };
        if rep.on_hover_text(aide).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            clic = Some(vue);
        }
    }
    clic
}

/// L'heure de la dernière lecture de l'entreprise, en gris comme le modèle (« Mis à jour à 15 h 40 ») : un point devant
/// quand ce n'est pas tout à fait à jour (ambre : d'un moment ; rouge : une lecture manquée ; gris : pas reliée), une
/// roue pendant qu'Eve lit ; coupée d'une ellipse à `largeur`.
fn mise_a_jour_dite(ui: &mut egui::Ui, f: &Fraicheur, largeur: f32) {
    let devant = if f.point == Point::Aucun { 0.0 } else { 18.0 };
    let encre = if f.point == Point::Rouge { erp::DANGER_ENCRE } else { erp::TEXTE_2 };
    let mut job = egui::text::LayoutJob::simple_singleline(f.texte.clone(), FontId::proportional(13.0), encre);
    job.wrap = egui::text::TextWrapping::truncate_at_width((largeur - devant).max(40.0));
    let g = ui.painter().layout_job(job);
    let (r, rep) = ui.allocate_exact_size(vec2(g.size().x + devant, 32.0), Sense::hover());
    let centre = pos2(r.left() + 6.0, r.center().y);
    match f.point {
        Point::Aucun => {}
        Point::Roue => roue(ui, centre, 5.5, erp::PRIMAIRE),
        point => {
            let c = match point {
                Point::Ambre => erp::AVERTISSEMENT,
                Point::Rouge => TRANCHES[2].2,
                _ => erp::TEXTE_3,
            };
            ui.painter().circle_filled(centre, 4.0, c);
        }
    }
    ui.painter().galley(pos2(r.left() + devant, r.center().y - g.size().y / 2.0), g, encre);
    rep.on_hover_text("Les chiffres de l'entreprise : Eve les relit dans Constructo AI toutes les dix minutes environ");
}

/// Le bouton qui relit Constructo AI : un rond blanc, la flèche qui tourne (ou une roue pendant qu'Eve lit) ; éteint
/// sans liaison.
fn bouton_relire(ui: &mut egui::Ui, permis: bool, lit: bool) -> egui::Response {
    let sens = if permis && !lit { Sense::click() } else { Sense::hover() };
    let (r, rep) = ui.allocate_exact_size(vec2(32.0, 32.0), sens);
    let survol = rep.hovered() && permis && !lit;
    let fond = if survol { erp::SURVOL } else { Color32::from_white_alpha(225) };
    ui.painter().circle(r.center(), 16.0, fond, Stroke::new(1.0, erp::FILET_DE_CARTE));
    if lit {
        roue(ui, r.center(), 6.5, erp::PRIMAIRE);
    } else {
        let encre = match (permis, survol) {
            (false, _) => erp::TEXTE_3.gamma_multiply(0.6),
            (true, true) => erp::TEXTE,
            (true, false) => erp::TEXTE_2,
        };
        let police = FontId::new(14.0, FontFamily::Name(ICONES.into()));
        ui.painter().text(r.center(), Align2::CENTER_CENTER, pictos::RELIRE, police, encre);
    }
    let aide = match (permis, lit) {
        (false, _) => "Relie d'abord Eve à Constructo AI",
        (true, true) => "Eve lit Constructo AI…",
        (true, false) => "Relire Constructo AI maintenant",
    };
    let rep = rep.on_hover_text(aide);
    if permis && !lit { rep.on_hover_cursor(egui::CursorIcon::PointingHand) } else { rep }
}

/// La roue d'une attente : un arc de trois quarts qui tourne (immobile quand Windows ne veut pas d'animations).
fn roue(ui: &egui::Ui, centre: Pos2, rayon: f32, couleur: Color32) {
    let vivant = theme::animations(ui.ctx());
    let debut = if vivant { (ui.input(|i| i.time) * 5.0) as f32 } else { 0.0 };
    let points: Vec<Pos2> = (0..=28)
        .map(|k| {
            let a = debut + k as f32 / 28.0 * 4.7;
            centre + vec2(a.cos(), a.sin()) * rayon
        })
        .collect();
    ui.painter().circle_stroke(centre, rayon, Stroke::new(2.0, couleur.gamma_multiply(0.18)));
    ui.painter().add(Shape::line(points, Stroke::new(2.0, couleur)));
    if vivant {
        ui.ctx().request_repaint();
    }
}

/// Une puce de l'invitation : l'icône et le mot dans un badge pastel de leur teinte.
fn puce(ui: &mut egui::Ui, icone: char, mot: &str, teinte: erp::Teinte) {
    let p = ui.painter().clone();
    let g_icone = p.layout_no_wrap(icone.to_string(), FontId::new(12.0, FontFamily::Name(ICONES.into())), teinte.encre);
    let g_mot = p.layout_no_wrap(mot.to_string(), FontId::new(12.5, FontFamily::Name(SEMI.into())), teinte.encre);
    let taille = vec2(12.0 + g_icone.size().x + 6.0 + g_mot.size().x + 12.0, 26.0);
    let (r, _) = ui.allocate_exact_size(taille, Sense::hover());
    p.rect_filled(r, 13.0, teinte.pastel.gamma_multiply(0.2));
    let l_icone = g_icone.size().x;
    p.galley(pos2(r.left() + 12.0, r.center().y - g_icone.size().y / 2.0), g_icone, teinte.encre);
    p.galley(pos2(r.left() + 12.0 + l_icone + 6.0, r.center().y - g_mot.size().y / 2.0), g_mot, teinte.encre);
}

/// Un bandeau au-dessus des chiffres, comme les alertes de l'ERP (`.erp-alert-*`) : ambre pour une lecture qui n'a pas
/// abouti (`avertissement`), bleu pâle pour une liaison perdue ; son icône, son texte, et son bouton contre le bord
/// droit (dessous, dans une place étroite). Rend vrai si le bouton est cliqué.
fn bandeau(ui: &mut egui::Ui, avertissement: bool, texte: &str, (mot, icone): (&str, char)) -> bool {
    let (fond, bord, encre, picto) = if avertissement {
        (erp::AVERTISSEMENT_FOND, erp::AVERTISSEMENT_BORD, erp::AVERTISSEMENT_ENCRE, pictos::ALERTES)
    } else {
        (Color32::from_rgb(0xEF, 0xF6, 0xFF), Color32::from_rgb(0xBF, 0xDB, 0xFE), erp::MARINE, pictos::INFO)
    };
    let largeur = ui.available_width();
    let p = ui.painter().clone();
    let bouton = theme::taille_du_bouton_erp(&p, Some(icone), mot, BoutonErp::Secondaire);
    let (marge_x, marge_y, l_icone) = (14.0, 10.0, 26.0);
    let a_cote = largeur >= 460.0;
    let l_texte = largeur - 2.0 * marge_x - l_icone - if a_cote { bouton.x + 14.0 } else { 0.0 };
    let g = p.layout(texte.to_string(), FontId::proportional(13.5), erp::TEXTE, l_texte.max(80.0));
    let h_contenu = if a_cote { g.size().y.max(bouton.y) } else { g.size().y + 10.0 + bouton.y };
    let (r, _) = ui.allocate_exact_size(vec2(largeur, h_contenu + 2.0 * marge_y), Sense::hover());
    p.rect(r, 10.0, fond, Stroke::new(1.0, bord), StrokeKind::Inside);
    let haut = r.top() + marge_y;
    let y_texte = if a_cote { haut + (h_contenu - g.size().y) / 2.0 } else { haut };
    let ligne = g.rows.first().map_or(g.size().y, |l| l.rect().height());
    let police = FontId::new(15.0, FontFamily::Name(ICONES.into()));
    p.text(pos2(r.left() + marge_x + 8.0, y_texte + ligne / 2.0), Align2::CENTER_CENTER, picto, police, encre);
    let h_texte = g.size().y;
    p.galley(pos2(r.left() + marge_x + l_icone, y_texte), g, erp::TEXTE);
    let place = if a_cote {
        Rect::from_min_size(pos2(r.right() - marge_x - bouton.x, haut + (h_contenu - bouton.y) / 2.0), bouton)
    } else {
        Rect::from_min_size(pos2(r.left() + marge_x + l_icone, haut + h_texte + 10.0), bouton)
    };
    let mut enfant = ui.new_child(UiBuilder::new().max_rect(place).layout(Layout::left_to_right(Align::Center)));
    theme::bouton_erp(&mut enfant, Some(icone), mot, BoutonErp::Secondaire).clicked()
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn les_vues_suivent_le_reglage() {
        assert_eq!(VUES.len(), VUES_DU_TABLEAU.len());
        assert_eq!(Vue::de("entreprise"), Vue::Entreprise);
        assert_eq!(Vue::de("famille"), Vue::Famille);
        assert_eq!(Vue::de("tout"), Vue::Tout);
        assert_eq!(Vue::de("n'importe quoi"), Vue::Tout, "un nom inconnu : tout");
        for (k, (nom, _)) in VUES_DU_TABLEAU.iter().enumerate() {
            assert_eq!(Vue::de(nom).rang(), k);
        }
    }

    #[test]
    fn les_montants_a_la_quebecoise() {
        assert_eq!(dollars(12_000.0), "12\u{202f}000\u{a0}$");
        assert_eq!(dollars(1_234.5), "1\u{202f}235\u{a0}$", "arrondi au dollar");
        assert_eq!(dollars(0.0), "0\u{a0}$");
        assert_eq!(dollars_court(450_000.0), "450\u{202f}000\u{a0}$");
        assert_eq!(dollars_court(1_250_000.0), "1,25\u{a0}M$");
        assert_eq!(dollars_court(12_500_000.0), "12,5\u{a0}M$");
        assert_eq!(dollars_court(3_000_000.0), "3\u{a0}M$");
        assert_eq!(dollars_court(999_999.0), "1\u{a0}M$", "presque un million : un million");
        assert_eq!(compte(1.0, "facture", "factures"), "1\u{a0}facture");
        assert_eq!(compte(4.0, "facture", "factures"), "4\u{a0}factures");
    }

    /// Le poste à l'heure de l'Est (−4 h) : 15 h 10 locale, 19 h 10 universelle, le 26 octobre 2026.
    fn maintenant() -> Maintenant {
        let jour = centre::numero_du_jour("2026-10-26").unwrap();
        Maintenant { jour, local: jour * 86_400 + 15 * 3600 + 600, unix: jour * 86_400 + 19 * 3600 + 600 }
    }

    #[test]
    fn l_heure_d_une_lecture_et_son_age() {
        let m = maintenant();
        assert_eq!(m.age("2026-10-26T15:06:00-04:00").map(|a| a.0), Some(240));
        assert_eq!(m.age("2026-10-26T19:06:00Z").map(|a| a.0), Some(240), "en heure universelle");
        assert_eq!(m.age("2026-10-26 15:06:00").map(|a| a.0), Some(240), "sans fuseau : l'heure du poste");
        assert_eq!(m.age("2026-10-26T15:06:00.250-04:00").map(|a| a.0), Some(240), "les fractions de seconde");
        assert_eq!(m.age("2026-10-26T15:20:00-04:00").map(|a| a.0), Some(0), "une horloge en avance : à l'instant");
        assert_eq!(m.age("hier"), None);
        let dit = |lu_a: &str| m.age(lu_a).map(|(_, local)| quand_dit(local, &m)).unwrap();
        assert_eq!(dit("2026-10-26T15:06:00-04:00"), "à 15 h 06");
        assert_eq!(dit("2026-10-26T13:00:00-04:00"), "à 13 h");
        assert_eq!(dit("2026-10-25T18:05:00-04:00"), "hier à 18 h 05");
        assert_eq!(dit("2026-10-24T09:00:00-04:00"), "le 24 oct. à 9 h");
        assert_eq!(de_quand("à 15 h 06"), "de 15 h 06");
        assert_eq!(de_quand("hier à 18 h"), "d'hier à 18 h");
        assert_eq!(de_quand("le 24 oct. à 9 h"), "du 24 oct. à 9 h");
    }

    #[test]
    fn la_fraicheur_dit_la_liaison_puis_la_lecture() {
        let m = maintenant();
        let avec = |etat: &str, lu_a: &str| {
            let v = serde_json::json!({ "etat": etat, "lu_a": lu_a, "kpis": { "projets_en_cours": 3 } });
            Some(Ok(Arc::new(v)))
        };
        let dite = |l: Liaison, x: &Option<Result<Arc<Value>, String>>| fraicheur(l, x.as_ref(), &m);
        assert_eq!(dite(Liaison::NonReliee, &None).point, Point::Gris);
        assert_eq!(dite(Liaison::EnCours, &None).point, Point::Roue);
        assert_eq!(dite(Liaison::Reliee, &None).texte, "Première lecture de Constructo AI…");
        let a_jour = dite(Liaison::Reliee, &avec("a_jour", "2026-10-26T15:06:00-04:00"));
        assert_eq!(a_jour, Fraicheur { point: Point::Aucun, texte: "Mis à jour à 15 h 06".into() });
        let vieille = dite(Liaison::Reliee, &avec("a_jour", "2026-10-25T18:05:00-04:00"));
        assert_eq!(vieille, Fraicheur { point: Point::Ambre, texte: "Mis à jour hier à 18 h 05".into() });
        assert_eq!(dite(Liaison::Reliee, &avec("en_cours", "2026-10-26T13:00:00-04:00")).point, Point::Roue);
        let manquee = dite(Liaison::Reliee, &avec("erreur", "2026-10-26T13:00:00-04:00"));
        assert_eq!(manquee, Fraicheur { point: Point::Rouge, texte: "Lecture manquée · mis à jour à 13 h".into() });
        assert_eq!(dite(Liaison::Reliee, &Some(Err("ne se lit pas".into()))).point, Point::Rouge);
        let premiere = Some(Ok(Arc::new(serde_json::json!({ "etat": "en_cours" }))));
        assert_eq!(dite(Liaison::Reliee, &premiere).texte, "Première lecture de Constructo AI…");
    }

    #[test]
    fn la_liaison_vient_du_chef_et_de_la_lecture() {
        assert_eq!(Liaison::de(&Value::Null, None), Liaison::Absente, "un chef qui n'en dit rien : pas d'entreprise");
        let c = |permis: bool, etat: &str| serde_json::json!({ "permis": permis, "etat": etat });
        assert_eq!(Liaison::de(&c(false, "connectee"), None), Liaison::Absente);
        assert_eq!(Liaison::de(&c(true, "connectee"), None), Liaison::Reliee);
        assert_eq!(Liaison::de(&c(true, "en_cours"), None), Liaison::EnCours);
        assert_eq!(Liaison::de(&c(true, "deconnectee"), None), Liaison::NonReliee);
        let refus = Ok(Arc::new(serde_json::json!({ "etat": "non_connecte", "permis": false })));
        assert_eq!(
            Liaison::de(&c(true, "deconnectee"), Some(&refus)),
            Liaison::Absente,
            "pas permise : ne pas inviter"
        );
    }

    #[test]
    fn les_chiffres_de_l_entreprise() {
        assert!(chiffres_de_l_entreprise(&Value::Null).is_empty());
        let k = serde_json::json!({
            "projets_en_cours": 3, "a_recevoir": 45_000.0, "en_retard": 12_000.0, "factures_en_retard": 4,
            "devis_en_attente": 5, "pipeline": 1_250_000.0, "opportunites_ouvertes": 7
        });
        let c = chiffres_de_l_entreprise(&serde_json::json!({ "kpis": k }));
        let de = |cle: &str| c.iter().find(|(x, _)| *x == cle).map(|(_, ch)| ch.clone()).unwrap();
        assert_eq!((de("projets").valeur.as_str(), de("projets").etiquette.as_str()), ("3", "chantiers en cours"));
        assert_eq!(de("recevoir").valeur, "45\u{202f}000\u{a0}$");
        assert_eq!(de("retard").etiquette, "en retard · 4\u{a0}factures");
        assert!(de("retard").alerte, "du retard : la pastille rouge");
        assert_eq!(de("pipeline").valeur, "1,25\u{a0}M$");
        assert_eq!(de("pipeline").etiquette, "au pipeline · 7\u{a0}opportunités");
        assert!(!c.iter().any(|(x, _)| *x == "banque"), "un indicateur absent n'a pas de tuile");
    }

    #[test]
    fn les_cartes_de_l_entreprise() {
        let m = maintenant();
        let v = serde_json::json!({
            "kpis": { "en_retard": 7_000.0, "projets_en_cours": 9 },
            "alertes": [
                { "texte": "Un devis expire", "gravite": "info" },
                { "texte": "Heures manquantes le vendredi 23 octobre : Jo Démo", "gravite": "moyenne",
                  "sorte": "heures" },
                { "texte": "2 factures en retard : 7 000 $", "gravite": "haute", "sorte": "factures" }
            ],
            "factures_en_retard": [
                { "numero": "F-1", "client": "Client Exemple inc.", "solde": 5_000.0, "jours": 75,
                  "echeance": "2026-08-12" },
                { "numero": "F-2", "client": "Gestion Démo ltée", "solde": 2_000.0, "jours": 12 }
            ],
            "projets": [
                { "numero": "PROJ-2026-00012", "nom": "Résidence, 12 rue des Exemples", "valeur": 425_000.0 }
            ],
            "agenda": [ { "quand": "2026-10-20", "texte": "Passée" }, { "quand": "2026-10-27", "texte": "Visite" } ],
            "balance_agee": { "a_venir": 3_000.0, "0_30": 2_000.0, "61_90": 5_000.0, "total": 10_000.0 },
            "devis": []
        });
        let listes = listes_de_l_entreprise(&v, &m);
        let carte = |titre: &str| listes.iter().find(|(_, l)| l.titre == titre).cloned().unwrap();
        let (rang_des_alertes, alertes) = carte("Alertes");
        assert_eq!(rang_des_alertes, rang::ALERTES_HAUTES, "une alerte haute : en haut de la page");
        assert_eq!(
            alertes.rangees[0].bouts[0],
            theme::Bout::tete("Factures", erp::DANGER_ENCRE),
            "la plus grave d'abord"
        );
        let heures: Vec<&str> = alertes.rangees[1].bouts.iter().map(|b| b.texte.as_str()).collect();
        assert_eq!(heures, ["Heures", " manquantes le vendredi 23 octobre : Jo Démo"], "le mot de tête pas redit");
        assert_eq!(alertes.rangees[2].bouts[0].texte, "Info");
        let (_, factures) = carte("Factures en retard");
        assert_eq!(factures.pilule, Some((dollars(7_000.0), erp::ROUGE)));
        assert_eq!(factures.rangees[0].droite, dollars(5_000.0));
        assert_eq!(factures.rangees[0].detail, "75\u{a0}jours de retard · échue le 12 août");
        assert_eq!(factures.rangees[0].bouts[0].couleur, Some(erp::DANGER_ENCRE), "plus de deux mois : rouge");
        let (_, projets) = carte("Chantiers en cours");
        assert_eq!(projets.pilule.as_ref().map(|p| p.0.as_str()), Some("9"));
        assert_eq!(projets.rangees[0].detail, "PROJ-2026-00012", "un numéro long passe au détail");
        assert_eq!(projets.pied, "… et 8 autres chantiers");
        let (_, agenda) = carte("Agenda de l'entreprise");
        assert_eq!(agenda.rangees.len(), 1, "ce qui est passé ne se montre pas");
        assert_eq!(agenda.rangees[0].bouts[0].texte, "Demain");
        let (_, balance) = carte("Balance âgée");
        assert_eq!(balance.rangees.len(), 3, "les tranches vides se taisent");
        assert_eq!(balance.barre.len(), 5, "la barre garde ses cinq parts");
        assert!(listes.iter().all(|(_, l)| l.titre != "Devis en attente"), "une liste vide : pas de carte");
    }

    #[test]
    fn les_cartes_du_modele() {
        let m = maintenant();
        let v = serde_json::json!({
            "kpis": { "bons_a_commander": 1 },
            "projets": [ { "numero": "014", "nom": "Agrandissement", "adresse": "12 rue des Exemples" } ],
            "soumissions": [ { "numero": "DEV-2026-035", "nom": "Cuisine", "client": "Client Exemple inc.",
                               "statut": "Brouillon", "valeur": 26_000.0, "modifie": "2026-10-26" } ],
            "devis": [ { "numero": "DEV-2026-031", "nom": "Toiture" } ],
            "commandes": [
                { "sorte": "livraison", "numero": "LIV-8", "date": "2026-10-29", "heure": "08:30",
                  "texte": "Matériaux" },
                { "sorte": "a_commander", "numero": "BA-40", "date": "2026-10-28", "montant": 1_250.0,
                  "texte": "Revêtement du balcon" }
            ],
            "heures": {
                "semaine": "2026-10-26", "cette_semaine": 3.5, "semaine_passee": 85.5, "a_venir": 8.0,
                "employes_actifs": 3, "derniere_saisie": "2026-10-26T07:00",
                "manquantes": { "jour": "2026-10-23", "employes": ["Jo Démo"] },
                "employes": [
                    { "nom": "Alex Modèle", "cette_semaine": 3.5, "semaine_passee": 45.0, "au_travail": true,
                      "jours": [ { "jour": "2026-10-23", "heures": 9.0 }, { "jour": "2026-10-26", "heures": 3.5 } ] },
                    { "nom": "Jo Démo", "cette_semaine": 0.0, "semaine_passee": 40.5 }
                ],
                "par_projet": [ { "nom": "Agrandissement", "numero": "014", "heures": 3.5 } ]
            },
            "factures_recues": [
                { "numero": "FACT-210", "fournisseur": "Béton Exemple ltée", "projet": "Agrandissement",
                  "date": "2026-10-23", "montant": 3_449.25, "solde": 0.0, "statut": "PAYEE" },
                { "numero": "FACT-212", "fournisseur": "Quincaillerie Démo", "montant": 412.5, "solde": 412.5,
                  "statut": "ENVOYEE" }
            ]
        });
        assert!(a_des_chiffres(&serde_json::json!({ "heures": { "cette_semaine": 3.5 } })), "des heures seules");
        assert!(!a_des_chiffres(&serde_json::json!({ "etat": "en_cours", "kpis": {}, "commandes": [] })));
        let listes = listes_de_l_entreprise(&v, &m);
        let carte = |titre: &str| listes.iter().find(|(_, l)| l.titre == titre).map(|(_, l)| l.clone()).unwrap();
        let chantiers = carte("Chantiers en cours");
        assert_eq!(
            chantiers.rangees[0].bouts[1].texte, " Agrandissement (12 rue des Exemples)",
            "l'adresse suit le nom"
        );
        let soumissions = carte("Soumissions client");
        assert_eq!(soumissions.rangees[0].detail, "Client Exemple inc. · brouillon · modifiée le lun. 26 oct.");
        assert!(listes.iter().all(|(_, l)| l.titre != "Devis en attente"), "les soumissions comprennent les devis");
        let commandes = carte("Commandes et livraisons");
        assert_eq!(commandes.pilule, Some(("1 à commander".to_string(), erp::AMBRE)));
        assert_eq!(commandes.rangees[0].bouts[0].texte, "À commander", "ce qui est à commander d'abord");
        assert_eq!(commandes.rangees[1].detail, "LIV-8 · le jeu. 29 oct. à 8 h 30");
        let heures = carte("Heures de la semaine");
        assert_eq!(heures.pilule.as_ref().map(|p| p.0.as_str()), Some("3\u{a0}employés actifs"));
        let mots: Vec<&str> = heures.rangees.iter().map(|r| r.bouts[0].texte.as_str()).collect();
        assert_eq!(mots, ["Semaine du 26 oct.", "Semaine passée", "À vérifier", "Heures manquantes", "014"]);
        assert_eq!(heures.rangees[0].detail, "dernière saisie : lun. 26 oct. à 7 h");
        assert_eq!(heures.rangees[4].droite, "3,5\u{a0}h");
        let equipe = carte("Heures de l'équipe");
        assert_eq!(equipe.rangees[0].detail, "lun. 3,5 · au travail", "les jours de cette semaine seulement");
        assert_eq!(equipe.pilule.as_ref().map(|p| p.0.as_str()), Some("3\u{a0}employés"), "tous ceux qui ont pointé");
        assert_eq!(equipe.pied, "… et 1 autre employé", "la lecture n'en donne qu'une partie");
        assert_eq!(equipe.rangees[1].bouts[0].couleur, Some(erp::DANGER_ENCRE), "il lui manque des heures : rouge");
        let recues = carte("Factures reçues");
        assert_eq!(recues.rangees[0].droite, dollars(3_449.25), "payée : son montant");
        assert_eq!(recues.rangees[0].detail, "Agrandissement · du ven. 23 oct. · payée");
        assert_eq!((recues.rangees[1].droite.clone(), recues.rangees[1].detail.as_str()), (dollars(412.5), "à payer"));
        assert_eq!(heures_dites(36.25), "36,25\u{a0}h");
        assert_eq!(heures_dites(45.0), "45\u{a0}h");
        assert_eq!(heures_dites(1_204.5), "1\u{202f}204,5\u{a0}h", "les milliers");
        assert_eq!(heures_dites(0.0), "0\u{a0}h");
    }

    use crate::services::{Incident, Ligne, Reponses5xx, Statut};
    use crate::stripe::{Abonnements, Ouvertes, Somme, Virement};

    /// Un service et sa page d'état (inventée).
    fn service(nom: &str, etat: Result<Etat, String>) -> Statut {
        Statut { nom: nom.into(), page: format!("https://status.{}.exemple", nom.to_lowercase()), etat }
    }

    fn va() -> Result<Etat, String> {
        Ok(Etat { niveau: Niveau::Operationnel, incidents: Vec::new(), composants: Vec::new() })
    }

    /// Un service à ce niveau, son incident de cet impact commencé il y a 38 minutes, et ces composants touchés.
    fn en_incident(m: &Maintenant, niveau: Niveau, impact: Niveau, composants: &[&str]) -> Result<Etat, String> {
        let incident = Incident {
            nom: " Elevated errors ".into(),
            impact,
            statut: "identified".into(),
            depuis: m.unix - 38 * 60,
            lien: String::new(),
        };
        let composants = composants.iter().map(|c| (c.to_string(), impact)).collect();
        Ok(Etat { niveau, incidents: vec![incident], composants })
    }

    #[test]
    fn les_services_en_ligne() {
        let m = maintenant();
        let releve = |lu: i64, statuts: Vec<Statut>| Releve { lu_a: m.unix - lu, statuts, render: Ok(Vec::new()) };
        let carte = |r: &Releve| liste_des_services(r, &m).unwrap();
        assert!(liste_des_services(&releve(0, Vec::new()), &m).is_none(), "sans page : pas de carte");
        // Tout va : sous l'entreprise, la pilule verte ; chaque rangée ouvre sa page d'état.
        let (r, l) = carte(&releve(120, vec![service("Claude", va()), service("GitHub", va())]));
        assert_eq!(r, rang::EN_LIGNE);
        assert_eq!(l.pilule, Some(("Tout fonctionne".to_string(), erp::VERT)));
        assert_eq!(
            l.rangees[0].bouts,
            [theme::Bout::tete("Claude", tete(erp::VERT)), theme::Bout::texte(" Opérationnel")]
        );
        assert_eq!(l.rangees[1].lien.as_deref(), Some("https://status.github.exemple"));
        assert_eq!(l.pied, "Mis à jour à 15\u{a0}h\u{a0}08", "l'heure d'un seul tenant");
        // Un incident : en haut de la page ; sa gravité, où il en est, depuis quand, ses composants (une fois chacun).
        let incident = en_incident(&m, Niveau::Majeur, Niveau::Majeur, &["API", "API", "Console"]);
        let (r, l) = carte(&releve(120, vec![service("Claude", incident), service("GitHub", va())]));
        assert_eq!(r, rang::EN_LIGNE_QUI_PRESSE);
        assert_eq!(l.pilule, Some(("1\u{a0}incident".to_string(), erp::ORANGE)));
        assert_eq!(l.teinte, erp::ORANGE);
        assert_eq!(l.rangees[0].bouts[0], theme::Bout::tete("Claude", tete(erp::ORANGE)));
        assert_eq!(l.rangees[0].bouts[1].texte, " Elevated errors", "le nom de l'incident, ébarbé");
        let detail = "Panne partielle · cause identifiée · depuis\u{a0}14\u{a0}h\u{a0}32 · API et Console";
        assert_eq!(l.rangees[0].detail, detail);
        // Une panne majeure : le rouge foncé de ce qui presse.
        let critique = en_incident(&m, Niveau::Critique, Niveau::Critique, &[]);
        let (_, l) = carte(&releve(120, vec![service("Render", critique)]));
        assert_eq!(l.rangees[0].bouts[0].couleur, Some(erp::DANGER_ENCRE));
        assert_eq!(l.pilule.map(|p| p.1), Some(erp::ROUGE));
        // Une maintenance : plus bas, en bleu ; un service qui va mal sans incident le dit.
        let maintenance = en_incident(&m, Niveau::Maintenance, Niveau::Maintenance, &["Postgres"]);
        let (r, l) = carte(&releve(120, vec![service("Render", maintenance)]));
        assert_eq!((r, l.pilule), (rang::EN_LIGNE_A_SURVEILLER, Some(("Maintenance".to_string(), erp::BLEU))));
        assert_eq!(l.rangees[0].detail, "Maintenance · cause identifiée · depuis\u{a0}14\u{a0}h\u{a0}32 · Postgres");
        let ralenti = Ok(Etat {
            niveau: Niveau::Mineur,
            incidents: Vec::new(),
            composants: (1..=5).map(|k| (format!("C{k}"), Niveau::Mineur)).collect(),
        });
        let (r, l) = carte(&releve(120, vec![service("GitHub", ralenti)]));
        assert_eq!((r, l.pilule), (rang::EN_LIGNE_QUI_PRESSE, Some(("Perturbation".to_string(), erp::AMBRE))));
        assert_eq!(l.rangees[0].bouts[1].texte, " Perturbation mineure");
        assert_eq!(l.rangees[0].detail, "C1, C2, C3 et 2 autres");
        let quatre: Vec<String> = (1..=4).map(|k| format!("C{k}")).collect();
        assert_eq!(liste_coupee(&quatre), "C1, C2, C3 et 1 autre");
        // Les composants d'une même sorte dans plusieurs groupes (les régions d'une page) se disent ensemble.
        let regions = ["Web Services (Oregon)", "Web Services (Ohio)", "Cron Jobs (Oregon)", "Web Services (Ohio)"];
        let composants: Vec<(String, Niveau)> = regions.iter().map(|c| (c.to_string(), Niveau::Majeur)).collect();
        assert_eq!(composants_dits(&composants), "Web Services (Oregon et Ohio) et Cron Jobs (Oregon)");
        let cinq: Vec<(String, Niveau)> =
            (1..=5).map(|k| (format!("Web Services (Région {k})"), Niveau::Majeur)).collect();
        assert_eq!(composants_dits(&cinq), "Web Services (Région 1, Région 2, Région 3 et 2 autres)");
        // Une page qui ne se lit plus : sa rangée le dit, grise ; un relevé de plus de 15 minutes, au pied.
        let (r, l) = carte(&releve(20 * 60, vec![service("Claude", Err("la page ne répond pas".into()))]));
        assert_eq!((r, l.pilule), (rang::EN_LIGNE, Some(("Lecture manquée".to_string(), erp::GRIS))));
        assert_eq!(l.rangees[0].bouts[1].texte, " Lecture manquée");
        assert_eq!(l.rangees[0].detail, "la page ne répond pas");
        assert_eq!(l.pied, "Lecture manquée · mis à jour à 14\u{a0}h\u{a0}50");
        assert_eq!(statut_d_incident("monitoring"), "sous surveillance");
        assert_eq!(statut_d_incident("in_progress"), "en cours");
        assert_eq!(statut_d_incident("tout_nouveau"), "tout nouveau", "un mot inconnu passe tel quel");
    }

    #[test]
    fn les_erreurs_de_render() {
        let m = maintenant();
        let ressource = |nom: &str, sorte, nombre, derniere_heure, derniere: Option<Ligne>| Erreurs {
            nom: nom.into(),
            sorte,
            nombre,
            plus: false,
            derniere_heure,
            derniere,
            reponses_5xx: Default::default(),
        };
        let ligne = |il_y_a: i64, message: &str| Some(Ligne { quand: m.unix - il_y_a, message: message.into() });
        assert!(liste_des_erreurs(&Ok(Vec::new()), m.unix, &m).is_none(), "rien sur Render : pas de carte");
        // Aucune erreur : sous l'entreprise, en vert.
        let aucune = Ok(vec![
            ressource(" app-exemple", Sorte::Web, 0, 0, None),
            ressource("base-exemple", Sorte::Base, 0, 0, None),
        ]);
        let (r, l) = liste_des_erreurs(&aucune, m.unix - 60, &m).unwrap();
        assert_eq!((r, l.pilule), (rang::EN_LIGNE, Some(("Aucune".to_string(), erp::VERT))));
        assert_eq!(l.rangees[0].bouts[0], theme::Bout::tete("app-exemple", tete(erp::VERT)), "le nom ébarbé");
        assert_eq!((l.rangees[1].bouts[1].texte.as_str(), l.rangees[1].droite.as_str()), (" Base de données", "0"));
        assert_eq!(l.pied, "Les dernières 24 heures · mis à jour à 15\u{a0}h\u{a0}09");
        // Des erreurs dans la dernière heure : en haut de la page, en rouge ; la plus récente, et sa ligne.
        let erreurs = Ok(vec![
            ressource("api-exemple", Sorte::Web, 14, 3, ligne(4 * 60, "  TypeError: x is undefined \n")),
            ressource("base-exemple", Sorte::Base, 2, 0, ligne(26 * 3600, "ERROR: timeout")),
        ]);
        let (r, l) = liste_des_erreurs(&erreurs, m.unix - 60, &m).unwrap();
        assert_eq!((r, l.pilule), (rang::EN_LIGNE_QUI_PRESSE, Some(("16\u{a0}erreurs".to_string(), erp::ROUGE))));
        assert_eq!(l.rangees[0].bouts[0].couleur, Some(erp::DANGER_ENCRE));
        assert_eq!(l.rangees[0].droite, "14");
        assert_eq!(l.rangees[0].detail, "3 dans la dernière heure · la dernière à 15\u{a0}h\u{a0}06");
        assert_eq!(l.rangees[0].extrait, "TypeError: x is undefined");
        assert_eq!(l.rangees[1].bouts[0].couleur, Some(tete(erp::AMBRE)), "des erreurs dans la journée : ambre");
        assert_eq!(l.rangees[1].detail, "la dernière hier à 13\u{a0}h\u{a0}10");
        // Des erreurs de la journée seulement : plus bas, en ambre ; davantage que la lecture n'en a compté.
        let mut beaucoup = ressource("api-exemple", Sorte::Web, 100, 0, ligne(5 * 3600, "Error"));
        beaucoup.plus = true;
        let (r, l) = liste_des_erreurs(&Ok(vec![beaucoup]), m.unix, &m).unwrap();
        assert_eq!((r, l.pilule), (rang::EN_LIGNE_A_SURVEILLER, Some(("100+\u{a0}erreurs".to_string(), erp::AMBRE))));
        assert_eq!(l.rangees[0].droite, "100+");
        assert_eq!(compte_lu(1, false, "erreur", "erreurs"), "1\u{a0}erreur");
        // Des réponses HTTP 5xx, sans erreur : elles comptent à part, dans la pilule et le détail, et leur ligne se
        // montre ; une dans la dernière heure, et la carte monte.
        let mut web = ressource("app-exemple", Sorte::Web, 0, 0, None);
        web.reponses_5xx =
            Reponses5xx { nombre: 2, plus: false, derniere_heure: 1, derniere: ligne(10 * 60, "HTTP 502 · GET /api") };
        let (r, l) = liste_des_erreurs(&Ok(vec![web]), m.unix, &m).unwrap();
        assert_eq!((r, l.pilule), (rang::EN_LIGNE_QUI_PRESSE, Some(("2\u{a0}réponses 5xx".to_string(), erp::ROUGE))));
        assert_eq!(l.rangees[0].bouts[0].couleur, Some(erp::DANGER_ENCRE));
        assert_eq!(l.rangees[0].detail, "2\u{a0}réponses HTTP 5xx, dont 1 dans la dernière heure");
        assert_eq!(l.rangees[0].extrait, "HTTP 502 · GET /api");
        // Une lecture de Render manquée : sa raison.
        let (r, l) = liste_des_erreurs(&Err("la clé est refusée (HTTP 401)".into()), m.unix, &m).unwrap();
        assert_eq!((r, l.pilule), (rang::EN_LIGNE_A_SURVEILLER, Some(("Lecture manquée".to_string(), erp::AMBRE))));
        assert_eq!(l.rangees[0].bouts[0].texte, "La clé est refusée (HTTP 401).");
        // Sa commande d'un seul tenant : une ligne ne la coupe pas.
        let refus = Err("la clé de Render est refusée (HTTP 401) — reposer la clé : eve --cle render".into());
        let l = liste_des_erreurs(&refus, m.unix, &m).unwrap().1;
        let attendu = "La clé de Render est refusée (HTTP 401) — reposer la clé : eve\u{a0}--cle\u{a0}render.";
        assert_eq!(l.rangees[0].bouts[0].texte, attendu);
    }

    /// Un compte Stripe inventé, lu il y a trois minutes : un paiement de 49,99 $ il y a dix minutes.
    fn compte_stripe(m: &Maintenant) -> Montants {
        Montants {
            lu_a: m.unix - 180,
            devise: "cad".into(),
            disponible: 3_410.27,
            en_attente: 1_256.4,
            aujourdhui: Somme { montant: 348.0, nombre: 2, plus: false },
            ce_mois: Somme { montant: 8_921.5, nombre: 41, plus: true },
            abonnements: Some(Abonnements { actifs: 27, mensuel: 4_263.0 }),
            virement: Some(Virement { montant: 2_184.6, arrivee: "2026-10-27".into(), fait: false }),
            ouvertes: Some(Ouvertes { nombre: 3, plus: false, du: 647.0, echues: 1 }),
            paiements: vec![Paiement {
                montant: 49.99,
                quand: m.unix - 600,
                qui: "Client Exemple inc.".into(),
                issue: Issue::Reussi,
            }],
        }
    }

    #[test]
    fn les_montants_de_stripe() {
        let m = maintenant();
        let listes = listes_de_stripe(&Ok(compte_stripe(&m)), &m);
        let (r, l) = &listes[0];
        assert_eq!(*r, rang::STRIPE);
        assert_eq!(l.pilule, Some((format!("{} ce mois-ci", dollars(8_921.5)), erp::INDIGO)));
        let mots: Vec<&str> = l.rangees.iter().map(|r| r.bouts[0].texte.as_str()).collect();
        assert_eq!(mots, ["Aujourd'hui", "Ce mois-ci", "Abonnements", "Solde", "Virement", "Factures"]);
        assert_eq!(l.rangees[0].droite, dollars(348.0), "un total s'arrondit au dollar");
        assert_eq!(l.rangees[1].bouts[1].texte, " au moins 41\u{a0}paiements", "une lecture arrêtée avant la fin");
        assert_eq!(l.rangees[2].droite, format!("{}/mois", dollars(4_263.0)));
        assert_eq!(l.rangees[3].detail, format!("{} en attente", dollars(1_256.4)));
        assert_eq!(l.rangees[4].bouts[1].texte, " arrive demain");
        assert_eq!(l.rangees[5].bouts[1].texte, " 3\u{a0}ouvertes, dont 1\u{a0}échue");
        assert_eq!(l.rangees[5].bouts[0].couleur, Some(tete(erp::AMBRE)), "une facture échue : ambre, sans alarme");
        // Des dizaines de vieilles factures, toutes échues : dites, la carte à son rang.
        let mut vieilles = compte_stripe(&m);
        vieilles.ouvertes = Some(Ouvertes { nombre: 42, plus: false, du: 12_345.0, echues: 42 });
        let (r, l) = listes_de_stripe(&Ok(vieilles.clone()), &m).remove(0);
        assert_eq!((r, l.rangees[5].bouts[1].texte.as_str()), (rang::STRIPE, " 42\u{a0}ouvertes, toutes échues"));
        assert_eq!(l.rangees[5].droite, dollars(12_345.0));
        vieilles.ouvertes = Some(Ouvertes { nombre: 1, plus: false, du: 49.0, echues: 1 });
        let l = listes_de_stripe(&Ok(vieilles), &m).remove(0).1;
        assert_eq!(l.rangees[5].bouts[1].texte, " 1\u{a0}ouverte, échue");
        assert_eq!(l.pied, "Avant les frais de Stripe · mis à jour à 15\u{a0}h\u{a0}07");
        let (r, paiements) = &listes[1];
        assert_eq!((*r, paiements.pilule.clone()), (rang::STRIPE, Some(("Tous réussis".to_string(), erp::VERT))));
        assert_eq!(paiements.rangees[0].droite, "49,99\u{a0}$", "un paiement garde ses cents");
        assert_eq!(paiements.rangees[0].detail, "aujourd'hui à 15\u{a0}h");
        // Un paiement échoué ce matin : la carte des paiements monte, rouge ; échoué il y a deux jours, elle reste.
        let mut s = compte_stripe(&m);
        let echoue =
            |il_y_a: i64| Paiement { montant: 99.0, quand: m.unix - il_y_a, qui: " ".into(), issue: Issue::Echoue };
        s.paiements.insert(0, echoue(3 * 3600));
        let (r, paiements) = listes_de_stripe(&Ok(s.clone()), &m).remove(1);
        assert_eq!((r, paiements.teinte), (rang::EN_LIGNE_QUI_PRESSE, erp::ROUGE));
        assert_eq!(paiements.pilule, Some(("1\u{a0}échoué".to_string(), erp::ROUGE)));
        assert_eq!(paiements.rangees[0].bouts[0], theme::Bout::tete("Échoué", erp::DANGER_ENCRE));
        assert_eq!(paiements.rangees[0].bouts[1].texte, " Client", "sans nom : « Client »");
        s.paiements[0] = echoue(2 * 86_400);
        assert_eq!(listes_de_stripe(&Ok(s.clone()), &m)[1].0, rang::STRIPE);
        // Des montants de plus de 25 minutes : une lecture manquée, au pied ; une lecture manquée sans montants : sa
        // raison ; un compte sans rien : pas de carte.
        s.lu_a = m.unix - 30 * 60;
        let pied = listes_de_stripe(&Ok(s), &m)[0].1.pied.clone();
        assert_eq!(pied, "Avant les frais de Stripe · lecture manquée · mis à jour à 14\u{a0}h\u{a0}40");
        let refus = "la clé d'API est refusée (invalide, expirée ou révoquée) : la reposer par « eve --cle stripe »";
        let manquee = listes_de_stripe(&Err(refus.into()), &m);
        assert_eq!(manquee.len(), 1);
        assert_eq!(manquee[0].1.pilule, Some(("Lecture manquée".to_string(), erp::AMBRE)));
        let attendu = "La clé d'API est refusée (invalide, expirée ou révoquée) : la reposer par \
                       «\u{a0}eve\u{a0}--cle\u{a0}stripe\u{a0}».";
        assert_eq!(manquee[0].1.rangees[0].bouts[0].texte, attendu, "la commande et ses guillemets d'un tenant");
        let vide = Montants {
            disponible: 0.0,
            en_attente: 0.0,
            aujourdhui: Somme::default(),
            ce_mois: Somme::default(),
            abonnements: None,
            virement: None,
            ouvertes: None,
            paiements: Vec::new(),
            ..compte_stripe(&m)
        };
        assert!(listes_de_stripe(&Ok(vide), &m).is_empty());
    }

    #[test]
    fn les_montants_dans_leur_devise() {
        assert_eq!(montant_stripe(8_921.5, "cad", false), "8\u{202f}922\u{a0}$");
        assert_eq!(montant_stripe(1_234.56, "CAD", true), "1\u{202f}234,56\u{a0}$");
        assert_eq!(montant_stripe(50.0, "cad", true), "50\u{a0}$", "un montant rond, sans cents");
        assert_eq!(montant_stripe(-49.99, "cad", true), "-49,99\u{a0}$");
        assert_eq!(montant_stripe(120.0, "usd", false), "120\u{a0}$\u{a0}US");
        assert_eq!(montant_stripe(75.0, "eur", false), "75\u{a0}€");
        assert_eq!(montant_stripe(300.0, "chf", false), "300\u{a0}CHF");
        assert_eq!(paiements_dits(0, false), "aucun paiement");
        assert_eq!(paiements_dits(1, false), "1\u{a0}paiement");
        let m = maintenant();
        assert_eq!(jour_dit("2026-10-26", &m), "aujourd'hui");
        assert_eq!(jour_dit("2026-10-25", &m), "hier");
        assert_eq!(jour_dit("2026-10-29", &m), "le jeu. 29 oct.");
    }

    #[test]
    fn les_cartes_en_ligne_ensemble() {
        let m = maintenant();
        assert!(listes_en_ligne(None, None, &m).is_empty(), "sans clé (aucun relevé) : aucune carte");
        let releve = Releve { lu_a: m.unix, statuts: vec![service("Claude", va())], render: Ok(Vec::new()) };
        let titres: Vec<String> = listes_en_ligne(Some(&releve), Some(&Ok(compte_stripe(&m))), &m)
            .into_iter()
            .map(|(_, l)| l.titre)
            .collect();
        assert_eq!(
            titres,
            ["Services en ligne", "Stripe", "Paiements Stripe"],
            "rien sur Render : pas de carte d'erreurs"
        );
    }
}
