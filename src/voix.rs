//! La voix : deux moteurs, au choix dans la fenêtre, et une file qui les fait parler sans trou.
//!
//! - **Piper** (neuronal, sur le poste, gratuit, hors ligne) : Jessica — la voix par défaut — et Siwis,
//!   deux voix de femme naturelles (accent de France), Pierre pour une voix d'homme. Un seul processus, lancé dès le
//!   démarrage (son modèle met une à deux secondes à se charger) et gardé ouvert : une ligne JSON par phrase, un WAV
//!   par ligne.
//! - **Windows** (WinRT, `Windows.Media.SpeechSynthesis`) : les voix du poste, dont celles du Canada français
//!   (Caroline, Nathalie, Claude) — toujours là, même sans Piper : c'est le repli quand il flanche.
//!
//! Le chef donne chaque phrase dès que l'Adjointe l'écrit (`dire`, des identifiants croissants). Le fil de la voix ne
//! s'arrête sur aucune : il l'écrit aussitôt à Piper, lit ses WAV à mesure qu'ils arrivent et les joue dans l'ordre
//! par deux lecteurs de Windows (`MediaPlayer`, qui jouent seuls pendant que le fil veille) — Piper prépare la phrase
//! suivante pendant que celle-ci se dit, l'autre lecteur l'ouvre d'avance, et elle part dans le silence final de
//! celle-ci. `taire` coupe par numéro tout ce qui a été donné jusque-là ; ce qui vient après se dit normalement.
//! `Evt::VoixFinie(id)` part une fois, et une seule, par identifiant, dans l'ordre : quand c'est dit, coupé ou en
//! échec ; avant lui, `Evt::VoixEntendue(id)` quand le premier son de la parole s'entend (rien si elle ne s'entend pas).

use std::collections::VecDeque;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use eve::parole::{self, Forme};

use crate::chef::Evt;
use crate::journal;
use crate::reglages::{Reglages, VOIX_PAR_DEFAUT, dossier_local};

use lecture::{Lecteur, Synthetiseur};
pub use prosodie::{Marque, Repere};

mod prosodie;

/// Le pas du fil quand la voix travaille : la file avance (et se coupe) au moins à ce rythme.
const PAS: Duration = Duration::from_millis(20);
/// Le pas du fil quand il n'y a rien à dire (un ordre le réveille aussitôt).
const PAS_AU_REPOS: Duration = Duration::from_millis(250);
/// La phrase en tête attend son WAV depuis ce temps : Piper ne répond plus.
const DELAI_PIPER: Duration = Duration::from_secs(30);
/// La synthèse d'une phrase par Windows ne répond plus.
const DELAI_WINDOWS: Duration = Duration::from_secs(20);
/// Piper annonce un WAV un instant avant de le fermer : on le relit jusqu'à ce qu'il soit complet, au plus ce temps.
const DELAI_WAV: Duration = Duration::from_secs(2);
/// Un son qui ne démarre pas dans ce délai est en échec.
const DELAI_DEMARRAGE: Duration = Duration::from_secs(5);
/// Le son suivant part à ce temps de la fin du précédent, dans son silence final : le lecteur de Windows met de 60 à
/// 120 ms à le faire entendre (mesuré le 2026-10-22), et la pause entre les phrases reste celle que Piper a voulue.
const AVANCE: Duration = Duration::from_millis(120);
/// Les WAV d'une séance précédente (plus vieux que ceci) se suppriment au démarrage ; ceux d'une autre Eve qui
/// tournerait en même temps (un essai) sont plus jeunes.
const AGE_RESTE: Duration = Duration::from_secs(60);

/// Une voix du poste : son identifiant (« piper:fr_FR-upmc-medium#0 », « windows:Microsoft Caroline ») et son nom.
#[derive(Clone, Debug, Serialize)]
pub struct VoixDispo {
    pub id: String,
    pub nom: String,
    #[serde(skip)]
    pub femme: bool,
}

enum Ordre {
    Dire {
        id: u64,
        texte: String,
    },
    Choisir(String),
    /// Réveille le fil : le seuil de la coupure est dans [`Coupure`].
    Taire,
}

/// La coupure par numéro, partagée entre le chef et le fil de la voix : `taire` coupe tout identifiant donné jusque-là,
/// et rien de ce qui vient après.
#[derive(Default)]
struct Coupure {
    /// Le plus grand identifiant donné à `dire`.
    emis: AtomicU64,
    /// Tout identifiant inférieur ou égal est coupé.
    coupe: AtomicU64,
}

impl Coupure {
    fn emettre(&self, id: u64) {
        self.emis.fetch_max(id, Ordering::SeqCst);
    }

    fn couper(&self) {
        self.coupe.fetch_max(self.emis.load(Ordering::SeqCst), Ordering::SeqCst);
    }

    fn seuil(&self) -> u64 {
        self.coupe.load(Ordering::SeqCst)
    }
}

pub struct Voix {
    tx: Sender<Ordre>,
    coupure: Arc<Coupure>,
}

/// La piste de la bouche d'une phrase : le niveau de sa voix pas à pas ([`enveloppe`]) et, à chaque pas, l'ouverture de
/// la mâchoire et la forme des lèvres ([`piste_de`]) ; et, pour les gestes du visage, ses repères ([`Repere`] : ses
/// pauses, la fin de ses groupes de mots).
#[derive(Clone)]
pub struct Piste {
    niveaux: Arc<[f32]>,
    machoires: Arc<[f32]>,
    formes: Arc<[Forme]>,
    reperes: Arc<[Repere]>,
}

/// La bouche à un instant d'une phrase ([`bouche_a_venir`]) : le niveau de la voix (de 0 à 1), l'ouverture de la
/// mâchoire (de 0 à 1, dans la série de bouches de la forme : la voyelle et le niveau) et la forme des lèvres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Articulation {
    pub niveau: f32,
    pub machoire: f32,
    pub forme: Forme,
}

impl Piste {
    /// Le temps qu'elle dure.
    pub fn duree(&self) -> Duration {
        PAS_DE_L_ENVELOPPE * self.niveaux.len() as u32
    }
}

/// Ce qui s'entend — l'instant où le son commence, et sa piste —, posé par le fil de la voix quand il donne le son au
/// lecteur, recalé dès que l'horloge du lecteur dit quand il a vraiment commencé, ôté quand il finit. La fenêtre le lit
/// à chaque image, sans attendre l'état du chef (dix fois par seconde : trop peu pour des lèvres) : une seule voix
/// parle, une seule bouche la suit.
static S_ENTEND: Mutex<Option<(Instant, Piste)>> = Mutex::new(None);

/// Le retard de l'écran, en microsecondes : le temps qu'une image calculée met à se voir, que la fenêtre déclare
/// ([`regler_le_retard_de_l_ecran`]). Ce qui se lit de la voix est alors ce qui s'entendra quand l'image se verra. Rien
/// pour les essais : le film du portrait garde chaque image à l'instant où elle se calcule.
static RETARD_DE_L_ECRAN: AtomicU64 = AtomicU64::new(0);

/// La fenêtre déclare le temps que ses images mettent à se voir.
pub fn regler_le_retard_de_l_ecran(retard: Duration) {
    RETARD_DE_L_ECRAN.store(retard.as_micros() as u64, Ordering::Relaxed);
}

/// Où en sera le son qui commence à `debut` quand l'image calculée maintenant se verra ([`ecoule_a`]).
fn ecoule(debut: Instant) -> Option<Duration> {
    let retard = Duration::from_micros(RETARD_DE_L_ECRAN.load(Ordering::Relaxed));
    ecoule_a(Instant::now(), debut, retard)
}

/// Où en sera le son qui commence à `debut` quand l'image calculée à `maintenant` se verra, `retard` plus tard ;
/// `None` : il n'aura pas encore commencé.
fn ecoule_a(maintenant: Instant, debut: Instant, retard: Duration) -> Option<Duration> {
    (maintenant + retard).checked_duration_since(debut)
}

/// Le début du son qui joue, d'après l'horloge du lecteur : l'heure (`maintenant`) moins sa `position` — le son sort du
/// poste 0,8 ms après (mesuré le 2026-10-25), là où l'état « joue » de Windows arrive de 13 ms avant lui à 18 ms après.
/// La plus petite des estimations (`avant` : celle des lectures d'avant) : un fil en retard ne fait que reculer
/// l'heure lue, jamais la position. Une position nulle ne dit rien (le son n'est pas encore parti), ni une position
/// plus longue que le temps passé depuis `Play()` (`joue_depuis`) : celle du son d'avant, que le lecteur garderait.
#[cfg_attr(not(windows), allow(dead_code))]
fn debut_du_son(
    maintenant: Instant,
    joue_depuis: Duration,
    position: Duration,
    avant: Option<Instant>,
) -> Option<Instant> {
    let possible = !position.is_zero() && position <= joue_depuis + Duration::from_millis(5);
    let lu = if possible { maintenant.checked_sub(position) } else { None };
    match (avant, lu) {
        (Some(a), Some(l)) => Some(a.min(l)),
        (a, l) => a.or(l),
    }
}

/// La bouche à cet instant ([`bouche_a_venir`]) ; `None` : rien ne s'entend.
pub fn bouche_du_moment() -> Option<Articulation> {
    bouche_a_venir(Duration::ZERO)
}

/// La bouche dans `avance`, celle de ce qui s'entendra alors : les lèvres d'une vraie personne se placent avant que le
/// son ne sorte. Au début du son, l'avance se prend peu à peu (le double du temps écoulé) : la bouche ne saute pas ce
/// qui ouvre la phrase — les lèvres closes du « b » de « Bonjour ». Le niveau et la mâchoire, entre les milieux de deux
/// pas (chacun dit le son de ses 40 ms) ; la forme, celle du pas. `None` : rien ne s'entend, ou la phrase aura fini.
pub fn bouche_a_venir(avance: Duration) -> Option<Articulation> {
    lire_la_piste(|ecoule| ecoule + avance.min(ecoule))
}

/// La forme des lèvres dans `avance` tout entière, sans la mise en route de [`bouche_a_venir`] : là où va la bouche
/// (sur une consonne des lèvres, elle s'y prépare dès le début du son : le « b » de « Bonjour » se ferme arrondi).
pub fn forme_a_venir(avance: Duration) -> Option<Forme> {
    lire_la_piste(|ecoule| ecoule + avance).map(|a| a.forme)
}

/// La piste de ce qui s'entend, lue à l'instant que donne `instant` d'après le temps écoulé depuis le début du son
/// ([`ecoule`]).
fn lire_la_piste(instant: impl Fn(Duration) -> Duration) -> Option<Articulation> {
    let s = S_ENTEND.lock().ok()?;
    let (debut, piste) = s.as_ref()?;
    let t = instant(ecoule(*debut)?).as_secs_f32() / PAS_DE_L_ENVELOPPE.as_secs_f32();
    let forme = *piste.formes.get(t as usize)?;
    let x = (t - 0.5).max(0.0);
    let (i, f) = (x as usize, x.fract());
    let entre = |v: &[f32]| {
        let a = v.get(i).copied().unwrap_or(0.0);
        a + (v.get(i + 1).copied().unwrap_or(0.0) - a) * f
    };
    Some(Articulation { niveau: entre(&piste.niveaux), machoire: entre(&piste.machoires), forme })
}

/// Les repères de la phrase qui s'entend ([`Repere`]), et où elle en sera dans `avance` (en secondes depuis son début,
/// même passé sa fin) : les gestes du visage s'y accrochent. `None` : rien ne s'entend, ou pas encore.
pub fn reperes_a_venir(avance: Duration) -> Option<(f32, Arc<[Repere]>)> {
    let s = S_ENTEND.lock().ok()?;
    let (debut, piste) = s.as_ref()?;
    Some(((ecoule(*debut)? + avance).as_secs_f32(), piste.reperes.clone()))
}

/// Fait entendre une piste à partir de maintenant (rien : plus rien ne s'entend) ; les essais.
pub fn faire_entendre(piste: Option<Piste>) {
    entendre(piste.map(|p| (Instant::now(), p)));
}

/// Ce qui s'entend : une piste et l'instant où son son commence, même un peu plus tard (le lecteur vient de le
/// recevoir), ou rien.
fn entendre(piste: Option<(Instant, Piste)>) {
    if let Ok(mut s) = S_ENTEND.lock() {
        *s = piste;
    }
}

/// Le son qui s'entend a commencé à `debut` : l'horloge du lecteur le dit plus juste que l'instant où il l'a reçu.
fn recaler(debut: Instant) {
    if let Ok(mut s) = S_ENTEND.lock()
        && let Some((d, _)) = s.as_mut()
    {
        *d = debut;
    }
}

/// La piste d'une phrase dite, `wav` (PCM 16 bits), dont le texte est `texte` : son enveloppe ([`enveloppe`]) et ses
/// syllabes ([`piste_de`]).
pub fn piste_du_son(wav: &[u8], texte: &str) -> Piste {
    piste_de(enveloppe(wav), texte)
}

/// La piste d'une phrase : son niveau pas à pas, et ses syllabes ([`parole::syllabes`]) posées sur les crêtes de la
/// voix ([`syllabes_des_cretes`]). Chaque pas prend la forme des lèvres de la crête la plus proche ; la mâchoire va de
/// la voyelle d'une crête à celle de la suivante, ouverte selon le niveau de la voix ([`contraste`]) : elle s'ouvre sur
/// la voyelle et se referme sur la consonne. Les lèvres se ferment sur les m, b et p, la lèvre du bas passe sous les
/// dents sur les f et v (`Forme::Fermee`, `Forme::Dents`), et la mâchoire se referme avec elles : dans l'intervalle qui
/// mène à chaque crête, les syllabes qui y commencent — celle de la crête, et celles qu'aucune crête n'a prises —
/// tombent sur ses creux, dans l'ordre ([`creux_de_l_intervalle`]) ; les lèvres y prennent la consonne qui ouvre
/// chacune, ou celle qui finit le mot d'avant (« Sam rit »), jusqu'à ce que la voyelle suivante s'entende
/// ([`fermeture`]) — et juste après leur voyelle quand une pause les suit (« Sam. » : [`fermer_apres`]).
pub fn piste_de(niveaux: Arc<[f32]>, texte: &str) -> Piste {
    let syllabes = parole::syllabes(texte);
    let cretes = cretes(&niveaux);
    let n = niveaux.len();
    let de_la_crete = syllabes_des_cretes(&niveaux, &cretes, &syllabes);
    // La voyelle d'une crête : celle de sa syllabe ; une crête sans syllabe (un chiffre, que le texte ne dit pas en
    // lettres) s'ouvre à moitié.
    let voyelle =
        |c: usize| de_la_crete[c].map_or((Forme::Ouverte, 0.5), |k| (syllabes[k].forme, syllabes[k].machoire));
    let mut formes = vec![Forme::Ouverte; n];
    let mut machoires = vec![0.0f32; n];
    for i in 0..n {
        let suivante = cretes.partition_point(|&c| c < i);
        let ((forme, machoire), (forme_b, machoire_b), f) = match (suivante.checked_sub(1), cretes.get(suivante)) {
            (Some(a), Some(&b)) => (voyelle(a), voyelle(suivante), (i - cretes[a]) as f32 / (b - cretes[a]) as f32),
            (Some(a), None) => (voyelle(a), voyelle(a), 0.0),
            (None, Some(_)) => (voyelle(suivante), voyelle(suivante), 0.0),
            (None, None) => ((Forme::Ouverte, 0.5), (Forme::Ouverte, 0.5), 0.0),
        };
        formes[i] = if f <= 0.5 { forme } else { forme_b };
        machoires[i] = (machoire + (machoire_b - machoire) * f) * contraste(niveaux[i]);
    }
    let mut levres = |forme: Forme, (de, a): (usize, usize)| {
        for i in de..a {
            (formes[i], machoires[i]) = (forme, 0.0);
        }
    };
    // La crête précédente, et sa syllabe.
    let mut precedente: Option<(usize, usize)> = None;
    for (&crete, &k) in cretes.iter().zip(&de_la_crete) {
        let Some(k) = k else { continue };
        let (debut, premiere) = precedente.map_or((0, 0), |(p, kp)| (p + 1, kp + 1));
        let pause = precedente.is_some() && silence(&niveaux[debut..crete]);
        if let Some((p, kp)) = precedente
            && pause
            && let Some(forme) = syllabes[kp].apres
        {
            levres(forme, fermer_apres(&niveaux, p, crete));
        }
        if premiere <= k {
            let creux = creux_de_l_intervalle(&niveaux, debut, crete, k + 1 - premiere);
            for (j, &d) in (premiere..=k).zip(&creux) {
                // La consonne qui ouvre la syllabe, sinon celle qui finit le mot d'avant (pas par-dessus une pause).
                let fin_du_mot = j.checked_sub(1).filter(|_| !(pause && j == premiere)).and_then(|p| syllabes[p].apres);
                if let Some(forme) = syllabes[j].avant.or(fin_du_mot) {
                    levres(forme, fermeture(&niveaux, debut, d, crete));
                }
            }
        }
        precedente = Some((crete, k));
    }
    if let Some((p, kp)) = precedente
        && let Some(forme) = syllabes[kp].apres
    {
        levres(forme, fermer_apres(&niveaux, p, n));
    }
    let reperes = prosodie::reperes_de(&niveaux, texte).into();
    Piste { niveaux, machoires: machoires.into(), formes: formes.into(), reperes }
}

/// La part du niveau de la voix qui ouvre la mâchoire : rien sous 0,45 (une consonne, un souffle), toute sur une
/// voyelle forte — l'enveloppe, en décibels, écrase l'écart entre une voyelle et la consonne qui la suit ; ainsi étiré,
/// il referme la bouche entre deux syllabes.
fn contraste(niveau: f32) -> f32 {
    ((niveau - 0.45) / 0.55).clamp(0.0, 1.0)
}

/// Un silence dans la voix : 160 ms au moins sous 0,25 (−40 dB sous ses voyelles ; un souffle n'y compte pas).
fn silence(niveaux: &[f32]) -> bool {
    niveaux.split(|&v| v >= 0.25).any(|calme| calme.len() * PAS_DE_L_ENVELOPPE.as_millis() as usize >= 160)
}

/// Les creux de l'intervalle qui mène à une crête (du pas `debut` à la crête, exclue), où commencent les `m` syllabes
/// qu'il contient : ses minimums les plus profonds, autant que de syllabes, dans l'ordre ; s'il en a moins, chaque
/// syllabe prend celui qui tombe à sa place. Au bout d'un silence, son dernier pas : la voix repart juste après.
fn creux_de_l_intervalle(niveaux: &[f32], debut: usize, crete: usize, m: usize) -> Vec<usize> {
    let mut minimums: Vec<usize> = (debut..crete)
        .filter(|&i| (i == debut || niveaux[i] <= niveaux[i - 1]) && niveaux[i] < niveaux[i + 1])
        .collect();
    if minimums.is_empty() {
        minimums.push(debut);
    }
    minimums.sort_by(|&a, &b| niveaux[a].total_cmp(&niveaux[b]));
    minimums.truncate(m);
    minimums.sort_unstable();
    (0..m).map(|j| minimums[j * minimums.len() / m]).collect()
}

/// Les pas où les lèvres se ferment au creux `creux` (dans l'intervalle qui commence au pas `debut` et mène à la crête
/// `crete`) : du pas qui le précède jusqu'à ce que la voyelle qui suit s'entende — la voix à mi-chemin du creux au
/// sommet suivant —, deux pas au plus après lui ; deux pas au moins en tout : en moins de 80 ms, les lèvres ne se
/// rejoindraient pas. Au tout début de la phrase, deux pas même si la voyelle s'entend déjà : la bouche ne lit la voix
/// qu'à partir du son, et ne montrerait pas sinon les lèvres closes du « p » de « Pourquoi ».
fn fermeture(niveaux: &[f32], debut: usize, creux: usize, crete: usize) -> (usize, usize) {
    let sommet = (creux + 1..crete).find(|&i| niveaux[i] >= niveaux[i + 1]).unwrap_or(crete);
    let seuil = niveaux[creux] + 0.5 * (niveaux[sommet] - niveaux[creux]);
    let voyelle = (creux + 1..=sommet).find(|&i| niveaux[i] >= seuil).unwrap_or(sommet);
    let de = creux.saturating_sub(1).max(debut);
    let borne = if de == 0 { crete.max(2).min(niveaux.len()) } else { crete };
    (de, voyelle.clamp(creux + 1, creux + 2).max(de + 2).min(borne))
}

/// Les pas où les lèvres se ferment après la syllabe de la crête `crete`, quand une pause la suit, avant le pas `fin` :
/// dès que sa voyelle baisse (de plus d'un quart, vers le creux qui suit : le m de « Sam. » est plus sourd que le a),
/// jusqu'au creux, deux pas au moins.
fn fermer_apres(niveaux: &[f32], crete: usize, fin: usize) -> (usize, usize) {
    let Some(creux) = (crete + 1..fin).min_by(|&a, &b| niveaux[a].total_cmp(&niveaux[b])) else { return (0, 0) };
    let seuil = niveaux[creux] + 0.75 * (niveaux[crete] - niveaux[creux]);
    let de = (crete + 1..=creux).find(|&i| niveaux[i] < seuil).unwrap_or(creux);
    (de, (creux + 1).max(de + 2).min(fin))
}

/// Les syllabes posées sur les crêtes de la voix : le rang de celle de chaque crête (aucune : le texte n'a pas de
/// syllabe pour elle). Les pauses de la voix — un [`silence`] entre deux crêtes — s'accordent d'abord à celles du
/// texte, sa ponctuation ([`accorder_les_pauses`]) ; d'une pause accordée à la suivante, les syllabes se répartissent
/// sur les crêtes, dans l'ordre — une par crête quand elles sont autant, sinon de la première à la dernière. Le texte
/// et la voix ne se décalent ainsi que d'une pause à l'autre, jamais sur toute la phrase.
fn syllabes_des_cretes(niveaux: &[f32], cretes: &[usize], syllabes: &[parole::Syllabe]) -> Vec<Option<usize>> {
    let (c, s) = (cretes.len(), syllabes.len());
    let mut de_la_crete = vec![None; c];
    if c == 0 || s == 0 {
        return de_la_crete;
    }
    let pauses_de_la_voix: Vec<usize> = (0..c - 1).filter(|&j| silence(&niveaux[cretes[j]..cretes[j + 1]])).collect();
    let pauses_du_texte: Vec<usize> = (0..s - 1).filter(|&k| syllabes[k].pause_apres).collect();
    let accords = accorder_les_pauses(&pauses_de_la_voix, c, &pauses_du_texte, s);
    let (mut c0, mut s0) = (0, 0);
    for (j, k) in accords.into_iter().chain([(c - 1, s - 1)]) {
        let (nc, ns) = (j + 1 - c0, k + 1 - s0);
        for i in 0..nc {
            de_la_crete[c0 + i] = Some(s0 + if nc == 1 { 0 } else { (i * (ns - 1) + (nc - 1) / 2) / (nc - 1) });
        }
        (c0, s0) = (j + 1, k + 1);
    }
    de_la_crete
}

/// Les pauses de la voix (après la crête de chaque rang, sur `c` crêtes) accordées à celles du texte (après la syllabe
/// de chaque rang, sur `s` syllabes) : autant de chaque côté — Piper marque chaque ponctuation —, dans l'ordre ; sinon,
/// le plus grand nombre de paires, dans l'ordre, chacune à peu près au même endroit de la phrase (la part des crêtes et
/// celle des syllabes qui la précèdent, à 0,3 près) ; les autres pauses, de la voix ou du texte, ne comptent pas.
fn accorder_les_pauses(voix: &[usize], c: usize, texte: &[usize], s: usize) -> Vec<(usize, usize)> {
    if voix.len() == texte.len() {
        return voix.iter().copied().zip(texte.iter().copied()).collect();
    }
    let place = |rang: usize, sur: usize| (rang + 1) as f32 / sur as f32;
    let (nv, nt) = (voix.len(), texte.len());
    // meilleur[a][b] : le meilleur accord des `a` premières pauses de la voix et des `b` premières du texte ; une paire
    // vaut 1, moins son écart.
    let mut meilleur = vec![vec![0.0f32; nt + 1]; nv + 1];
    for a in 1..=nv {
        for b in 1..=nt {
            let ecart = (place(voix[a - 1], c) - place(texte[b - 1], s)).abs();
            let paire = if ecart <= 0.3 { meilleur[a - 1][b - 1] + 1.0 - ecart } else { f32::NEG_INFINITY };
            meilleur[a][b] = meilleur[a - 1][b].max(meilleur[a][b - 1]).max(paire);
        }
    }
    let (mut a, mut b, mut paires) = (nv, nt, Vec::new());
    while a > 0 && b > 0 {
        if meilleur[a][b] == meilleur[a - 1][b] {
            a -= 1;
        } else if meilleur[a][b] == meilleur[a][b - 1] {
            b -= 1;
        } else {
            paires.push((voix[a - 1], texte[b - 1]));
            (a, b) = (a - 1, b - 1);
        }
    }
    paires.reverse();
    paires
}

/// Les crêtes de la voix — ses syllabes : les pas d'au moins 0,35 que leurs deux voisins de chaque côté ne dépassent
/// pas, à 120 ms l'une de l'autre au moins (la plus haute gardée).
fn cretes(n: &[f32]) -> Vec<usize> {
    let mut gardees: Vec<usize> = Vec::new();
    for i in 0..n.len() {
        let voisins = i.saturating_sub(2)..(i + 3).min(n.len());
        if n[i] < 0.35 || voisins.into_iter().any(|j| n[j] > n[i]) {
            continue;
        }
        match gardees.last_mut() {
            Some(d) if i - *d < 3 => {
                if n[i] > n[*d] {
                    *d = i;
                }
            }
            _ => gardees.push(i),
        }
    }
    gardees
}

impl Voix {
    /// Dit un texte (une phrase, ou plusieurs) après ce qui a été donné avant ; `Evt::VoixFinie(id)` quand c'est dit,
    /// coupé ou en échec.
    pub fn dire(&self, id: u64, texte: &str) {
        self.coupure.emettre(id);
        let _ = self.tx.send(Ordre::Dire { id, texte: texte.to_string() });
    }

    /// Se tait tout de suite : coupe ce qui se dit et tout ce qui a été donné jusqu'ici. Un `dire` qui suit se dit
    /// normalement.
    pub fn taire(&self) {
        self.coupure.couper();
        let _ = self.tx.send(Ordre::Taire);
    }

    pub fn choisir(&self, id: &str) {
        let _ = self.tx.send(Ordre::Choisir(id.to_string()));
    }
}

pub fn demarrer(reglages: &Reglages, rapport: Sender<Evt>) -> Voix {
    lancer(reglages, rapport, None)
}

fn lancer(reglages: &Reglages, rapport: Sender<Evt>, trace: Option<Sender<Trace>>) -> Voix {
    let (tx, rx) = mpsc::channel();
    let coupure = Arc::new(Coupure::default());
    let c = coupure.clone();
    let (voulue, debit, douceur) = (reglages.voix.clone(), reglages.debit, Douceur::de(reglages.douceur));
    thread::spawn(move || Fil::ouvrir(&voulue, debit, douceur, c, rapport, trace).mener(&rx));
    Voix { tx, coupure }
}

/// Le pas du niveau de la voix ([`enveloppe`]).
const PAS_DE_L_ENVELOPPE: Duration = Duration::from_millis(40);

/// Le niveau d'un WAV (PCM 16 bits), pas à pas ([`PAS_DE_L_ENVELOPPE`]) : de 0 sous −50 dBFS (le souffle de la
/// synthèse), 1 à −15 dBFS, plus au-dessus, en décibels comme l'oreille l'entend ; puis rapporté à la phrase (son
/// neuvième décile parmi ce qui s'entend vaut 1, et rien ne le dépasse) : une voix plus douce ou plus forte ouvre la
/// bouche du portrait autant. Les voyelles de Piper montent de −7 à −13 dBFS : écrêtées à −15 dBFS, elles ouvraient
/// toutes la bouche en grand, une syllabe comme la suivante.
fn enveloppe(wav: &[u8]) -> Arc<[f32]> {
    let Some(e) = entete(wav) else { return Arc::from([]) };
    let fin = (e.debut + e.taille).min(wav.len());
    let (echantillons, _) = wav[e.debut..fin].as_chunks::<2>();
    // Les échantillons d'un pas, toutes voies comprises : la cadence en octets dit le temps, quel que soit le WAV.
    let par_pas = (e.octets_par_seconde as usize / 2 * PAS_DE_L_ENVELOPPE.as_millis() as usize / 1000).max(1);
    let mut niveaux: Vec<f32> = echantillons
        .chunks(par_pas)
        .map(|morceau| {
            let carres: f64 = morceau
                .iter()
                .map(|&b| {
                    let v = f64::from(i16::from_le_bytes(b)) / 32_768.0;
                    v * v
                })
                .sum();
            let db = 20.0 * (carres / morceau.len() as f64).sqrt().max(1e-6).log10();
            ((db + 50.0) / 35.0).max(0.0) as f32
        })
        .collect();
    let mut entendus: Vec<f32> = niveaux.iter().copied().filter(|&v| v > 0.05).collect();
    if !entendus.is_empty() {
        entendus.sort_by(f32::total_cmp);
        let reference = entendus[(entendus.len() - 1) * 9 / 10];
        for v in &mut niveaux {
            *v = (*v / reference).min(1.0);
        }
    }
    niveaux.into()
}

/// Ce que fait la voix, pour l'essai sur le poste : le délai de la première phrase, les trous, la coupure.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug)]
enum Trace {
    PiperLance(Instant),
    PiperPret(Instant),
    PiperRelance(Instant),
    /// Le son est donné au lecteur…
    Debut {
        id: u64,
        a: Instant,
    },
    /// … et il s'entend (le lecteur joue).
    Son {
        id: u64,
        a: Instant,
    },
    /// Il est fini : maintenant, ou à la fin de son silence final quand le suivant part pendant ce silence.
    Fin {
        id: u64,
        a: Instant,
    },
    Coupee(Instant),
}

/// La douceur de la voix neuronale : la prosodie de Piper (`--noise_scale` : la variabilité de la voix ; `--noise_w` :
/// celle du rythme ; `--sentence_silence` : la pause entre les phrases), puis un passe-bas doux (deux pôles : les
/// sifflantes adoucies) et un gain un peu sous 1. Réglée à l'oreille (A, B).
#[derive(Clone, Copy)]
struct Douceur {
    bruit: f32,
    rythme: f32,
    silence: f32,
    /// La fréquence de coupure du passe-bas (Hz) ; 0 : aucun.
    coupure: f32,
    gain: f32,
}

impl Douceur {
    fn de(niveau: u8) -> Douceur {
        match niveau {
            0 => Douceur { bruit: 0.667, rythme: 0.8, silence: 0.2, coupure: 0.0, gain: 1.0 },
            // « Encore un peu plus douce » : une voix plus calme, des aigus plus adoucis. Le
            // rythme reste celui de Piper (0,8) : à 0,30, `noise_w` divisait la durée des sons par deux — 11,6 syllabes
            // par seconde, le double d'un débit naturel, et Whisper n'y comprenait plus rien (mesuré ; à l'écoute :
            // « robotique… ou trop vite »).
            1 => Douceur { bruit: 0.45, rythme: 0.8, silence: 0.35, coupure: 5000.0, gain: 0.85 },
            _ => Douceur { bruit: 0.40, rythme: 0.8, silence: 0.40, coupure: 4000.0, gain: 0.80 },
        }
    }
}

/// Où sont, dans un WAV, la cadence et les données.
struct Entete {
    taux: u32,
    octets_par_seconde: u32,
    debut: usize,
    taille: usize,
}

fn mot(o: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([o[i], o[i + 1], o[i + 2], o[i + 3]])
}

fn entete(wav: &[u8]) -> Option<Entete> {
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let (mut taux, mut octets_par_seconde) = (0, 0);
    let mut i = 12;
    while i + 8 <= wav.len() {
        let taille = mot(wav, i + 4) as usize;
        match &wav[i..i + 4] {
            b"fmt " if i + 20 <= wav.len() => {
                taux = mot(wav, i + 12);
                octets_par_seconde = mot(wav, i + 16);
            }
            b"data" => return Some(Entete { taux, octets_par_seconde, debut: i + 8, taille }),
            _ => {}
        }
        i = i.saturating_add(8 + taille + (taille & 1));
    }
    None
}

/// Un WAV mono, PCM 16 bits.
#[cfg_attr(not(windows), allow(dead_code))]
fn wav_mono(echantillons: &[i16], taux: u32) -> Vec<u8> {
    let taille = (echantillons.len() * 2) as u32;
    let mut o = Vec::with_capacity(44 + taille as usize);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&(36 + taille).to_le_bytes());
    o.extend_from_slice(b"WAVEfmt ");
    o.extend_from_slice(&16u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&taux.to_le_bytes());
    o.extend_from_slice(&(taux * 2).to_le_bytes());
    o.extend_from_slice(&2u16.to_le_bytes());
    o.extend_from_slice(&16u16.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&taille.to_le_bytes());
    for e in echantillons {
        o.extend_from_slice(&e.to_le_bytes());
    }
    o
}

/// Un WAV dont toutes les données sont là.
fn wav_complet(wav: &[u8]) -> bool {
    entete(wav).is_some_and(|e| wav.len() >= e.debut + e.taille)
}

/// Un WAV tronqué ramené à ce qui est là, l'en-tête à la même mesure ; faux si ce n'est pas un WAV.
fn ramener(wav: &mut Vec<u8>) -> bool {
    let Some(e) = entete(wav) else { return false };
    let taille = (wav.len() - e.debut).min(e.taille) & !1;
    wav.truncate(e.debut + taille);
    wav[e.debut - 4..e.debut].copy_from_slice(&(taille as u32).to_le_bytes());
    let riff = (wav.len() - 8) as u32;
    wav[4..8].copy_from_slice(&riff.to_le_bytes());
    true
}

/// Le silence à la fin d'un WAV (16 bits) : ce qui suit son dernier échantillon audible.
#[cfg_attr(not(windows), allow(dead_code))]
fn silence_final(wav: &[u8]) -> Duration {
    let Some(e) = entete(wav) else { return Duration::ZERO };
    let fin = (e.debut + e.taille).min(wav.len());
    let (echantillons, _) = wav[e.debut..fin].as_chunks::<2>();
    let total = echantillons.len();
    // Sous -42 dBFS : le souffle de la synthèse, pas la voix.
    let dernier = echantillons.iter().rposition(|&b| i16::from_le_bytes(b).unsigned_abs() > 250);
    let muets = dernier.map_or(total, |i| total - i - 1);
    if e.octets_par_seconde == 0 {
        return Duration::ZERO;
    }
    Duration::from_secs_f64((muets * 2) as f64 / f64::from(e.octets_par_seconde))
}

/// La durée d'un WAV.
fn duree_wav(wav: &[u8]) -> Option<Duration> {
    let e = entete(wav)?;
    let taille = e.taille.min(wav.len().saturating_sub(e.debut));
    (e.octets_par_seconde > 0).then(|| Duration::from_secs_f64(taille as f64 / f64::from(e.octets_par_seconde)))
}

/// Lit un WAV de Piper : `Ok(None)` tant qu'il n'est pas complet ; `fin` : ce qui est là, tel quel.
fn lire_wav(chemin: &Path, fin: bool) -> io::Result<Option<Vec<u8>>> {
    let mut wav = fs::read(chemin)?;
    if wav_complet(&wav) {
        return Ok(Some(wav));
    }
    if !fin {
        return Ok(None);
    }
    if ramener(&mut wav) { Ok(Some(wav)) } else { Err(io::Error::other("ce n'est pas un WAV")) }
}

/// Adoucit un WAV de Piper (PCM 16 bits), en mémoire : deux passes d'un passe-bas du premier ordre, et le gain.
fn adoucir(wav: &mut [u8], d: Douceur) {
    if d.coupure <= 0.0 && (d.gain - 1.0).abs() < f32::EPSILON {
        return;
    }
    let Some(e) = entete(wav) else { return };
    let fin = (e.debut + e.taille).min(wav.len());
    let (echantillons, _) = wav[e.debut..fin].as_chunks_mut::<2>();
    let mut x: Vec<f32> = echantillons.iter().map(|&b| f32::from(i16::from_le_bytes(b))).collect();
    let taux = e.taux as f32;
    if d.coupure > 0.0 && taux > 0.0 {
        let a = 1.0 - (-2.0 * std::f32::consts::PI * d.coupure / taux).exp();
        for _ in 0..2 {
            let mut s = 0.0f32;
            for v in x.iter_mut() {
                s += a * (*v - s);
                *v = s;
            }
        }
    }
    for (b, v) in echantillons.iter_mut().zip(x) {
        *b = ((v * d.gain).clamp(-32_768.0, 32_767.0) as i16).to_le_bytes();
    }
}

/// Piper et ses voix : dans le dossier local du poste (`piper\piper\piper.exe`, `voix\*.onnx` et leur `.onnx.json`, que
/// `Eve.bat` y télécharge), sinon dans `ressources\` du dossier d'Eve. Sans Piper, les voix de Windows.
fn dossier_piper() -> PathBuf {
    crate::reglages::ressource("piper").join("piper")
}

fn dossier_voix() -> PathBuf {
    crate::reglages::ressource("voix")
}

const FEMMES: [&str; 12] = [
    "jessica", "siwis", "caroline", "nathalie", "sylvie", "julie", "hortense", "denise", "brigitte", "eloise", "marie",
    "upmc",
];

/// Le nom d'une voix dans la liste : court (la rangée des réglages est étroite) — « Jessica », « Pierre (homme) »,
/// « Caroline (Windows) ».
fn libelle(nom: &str, moteur: &str, femme: bool) -> String {
    match (moteur, femme) {
        ("neuronale", true) => nom.to_string(),
        ("neuronale", false) => format!("{nom} (homme)"),
        (_, true) => format!("{nom} ({moteur})"),
        (_, false) => format!("{nom} ({moteur}, homme)"),
    }
}

fn majuscule(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|p| p.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// Les voix du poste : celles de Piper (les modèles de `voix\`), puis les voix françaises de Windows ; la voix par
/// défaut d'abord, puis les voix de femme.
pub fn lister() -> Vec<VoixDispo> {
    let mut l = Vec::new();
    if dossier_piper().join("piper.exe").is_file() {
        let mut modeles: Vec<PathBuf> = fs::read_dir(dossier_voix())
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "onnx")).collect())
            .unwrap_or_default();
        modeles.sort();
        for m in modeles {
            let tige = m.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let config: Value = fs::read_to_string(m.with_extension("onnx.json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or(Value::Null);
            let mut locuteurs: Vec<(String, u64)> = config
                .get("speaker_id_map")
                .and_then(Value::as_object)
                .map(|o| o.iter().filter_map(|(n, i)| i.as_u64().map(|i| (n.clone(), i))).collect())
                .unwrap_or_default();
            locuteurs.sort_by_key(|x| x.1);
            if locuteurs.is_empty() {
                // « fr_FR-siwis-medium » : le nom est au milieu.
                let nom = tige.split('-').nth(1).unwrap_or(&tige).to_string();
                let femme = FEMMES.contains(&nom.as_str());
                l.push(VoixDispo {
                    id: format!("piper:{tige}"),
                    nom: libelle(&majuscule(&nom), "neuronale", femme),
                    femme,
                });
            } else {
                for (nom, i) in locuteurs {
                    let femme = FEMMES.contains(&nom.as_str());
                    l.push(VoixDispo {
                        id: format!("piper:{tige}#{i}"),
                        nom: libelle(&majuscule(&nom), "neuronale", femme),
                        femme,
                    });
                }
            }
        }
    }
    l.extend(voix_windows());
    l.sort_by_key(|v| (v.id != VOIX_PAR_DEFAUT, !v.femme));
    l
}

/// La voix voulue si elle est là ; sinon la voix par défaut, une voix de femme, la première.
fn choisir_dans(liste: &[VoixDispo], voulue: &str) -> String {
    liste
        .iter()
        .find(|v| v.id == voulue)
        .or_else(|| liste.iter().find(|v| v.id == VOIX_PAR_DEFAUT))
        .or_else(|| liste.iter().find(|v| v.femme))
        .or_else(|| liste.first())
        .map(|v| v.id.clone())
        .unwrap_or_default()
}

/// Le repli quand Piper flanche : la première voix française de femme de Windows, à défaut une voix française de
/// Windows.
fn voix_de_repli(liste: &[VoixDispo]) -> Option<&VoixDispo> {
    let windows = || liste.iter().filter(|v| v.id.starts_with("windows:"));
    windows().find(|v| v.femme).or_else(|| windows().next())
}

/// Le texte en phrases : la première se dit pendant que Piper prépare la suivante.
pub fn decouper(texte: &str) -> Vec<String> {
    let car: Vec<char> = texte.chars().collect();
    let mut phrases = Vec::new();
    let mut courante = String::new();
    for (i, &c) in car.iter().enumerate() {
        courante.push(c);
        // Aux vraies fins de phrase seulement : couper à un point-virgule hache la phrase à l'oreille.
        let fin = matches!(c, '.' | '!' | '?' | '…') && car.get(i + 1).is_none_or(|n| n.is_whitespace());
        if fin && courante.trim().chars().count() >= 20 {
            phrases.push(courante.trim().to_string());
            courante.clear();
        }
    }
    if !courante.trim().is_empty() {
        phrases.push(courante.trim().to_string());
    }
    phrases
}

// ---- La file ------------------------------------------------------------------------------------------------------

/// Où en est un morceau de la file.
enum Etat<A> {
    /// À préparer par la voix choisie.
    AFaire,
    /// En préparation (chez Piper ou chez Windows) depuis cet instant.
    EnPreparation(Instant),
    Pret(A),
    /// Il se dit : c'est la tête de la file.
    EnCours,
    /// Rien à dire : vide, coupé ou en échec ; il se termine à son tour.
    Muet,
}

/// Une phrase d'une parole (un appel à `dire`), à sa place dans la file.
struct Morceau<A> {
    id: u64,
    /// Unique : il retrouve le morceau quand son son arrive.
    numero: u64,
    /// Le dernier morceau de sa parole : quand il est fini, la parole l'est.
    dernier: bool,
    texte: String,
    etat: Etat<A>,
}

/// Ce qui reste à dire, dans l'ordre. Rien n'en sort que par la tête : chaque parole se termine donc une fois, à son
/// tour, dans quelque ordre que ses sons arrivent. (Le son est générique : un WAV en mémoire sur le poste, un simple
/// nom dans les essais.)
struct File<A> {
    morceaux: VecDeque<Morceau<A>>,
    compteur: u64,
}

impl<A> File<A> {
    fn nouvelle() -> File<A> {
        File { morceaux: VecDeque::new(), compteur: 0 }
    }

    fn vide(&self) -> bool {
        self.morceaux.is_empty()
    }

    /// Une parole de plus, une phrase par morceau ; vide ou déjà coupée : un morceau muet, qui se termine à son tour.
    fn ajouter(&mut self, id: u64, texte: &str, coupee: bool) {
        let phrases = if coupee { Vec::new() } else { decouper(texte) };
        if phrases.is_empty() {
            self.compteur += 1;
            self.morceaux.push_back(Morceau {
                id,
                numero: self.compteur,
                dernier: true,
                texte: String::new(),
                etat: Etat::Muet,
            });
            return;
        }
        let n = phrases.len();
        for (i, texte) in phrases.into_iter().enumerate() {
            self.compteur += 1;
            self.morceaux.push_back(Morceau {
                id,
                numero: self.compteur,
                dernier: i + 1 == n,
                texte,
                etat: Etat::AFaire,
            });
        }
    }

    fn morceau(&mut self, numero: u64) -> Option<&mut Morceau<A>> {
        self.morceaux.iter_mut().find(|m| m.numero == numero)
    }

    /// Le morceau est encore à dire : ni coupé, ni en échec, ni fini.
    fn vivant(&self, numero: u64) -> bool {
        self.morceaux.iter().any(|m| m.numero == numero && !matches!(m.etat, Etat::Muet))
    }

    /// Coupe tout ce qui a un identifiant inférieur ou égal au seuil ; vrai si le morceau qui se dit en est.
    fn couper(&mut self, seuil: u64) -> bool {
        let mut lecture = false;
        for m in self.morceaux.iter_mut().filter(|m| m.id <= seuil) {
            lecture |= matches!(m.etat, Etat::EnCours);
            m.etat = Etat::Muet;
        }
        lecture
    }

    /// Les morceaux à préparer, dans l'ordre.
    fn a_faire(&self) -> Vec<(u64, String)> {
        self.morceaux.iter().filter(|m| matches!(m.etat, Etat::AFaire)).map(|m| (m.numero, m.texte.clone())).collect()
    }

    fn premier_a_faire(&self) -> Option<(u64, String)> {
        self.morceaux.iter().find(|m| matches!(m.etat, Etat::AFaire)).map(|m| (m.numero, m.texte.clone()))
    }

    fn en_preparation(&mut self, numero: u64, depuis: Instant) {
        if let Some(m) = self.morceau(numero)
            && matches!(m.etat, Etat::AFaire)
        {
            m.etat = Etat::EnPreparation(depuis);
        }
    }

    /// Le son d'un morceau ; perdu (faux) si le morceau a été coupé entre-temps.
    fn pret(&mut self, numero: u64, son: A) -> bool {
        match self.morceau(numero) {
            Some(m) if matches!(m.etat, Etat::AFaire | Etat::EnPreparation(_)) => {
                m.etat = Etat::Pret(son);
                true
            }
            _ => false,
        }
    }

    /// Un morceau en préparation repart à faire : la voix a changé, ou Piper a été relancé.
    fn reprendre(&mut self, numero: u64) {
        if let Some(m) = self.morceau(numero)
            && matches!(m.etat, Etat::EnPreparation(_))
        {
            m.etat = Etat::AFaire;
        }
    }

    /// Un morceau qui ne se dira pas (en échec) : il se termine à son tour.
    fn abandonner(&mut self, numero: u64) {
        if let Some(m) = self.morceau(numero)
            && !matches!(m.etat, Etat::EnCours)
        {
            m.etat = Etat::Muet;
        }
    }

    /// La tête (le premier morceau qui n'est pas muet) attend son son : son numéro, et depuis quand.
    fn tete_en_preparation(&self) -> Option<(u64, Instant)> {
        let m = self.morceaux.iter().find(|m| !matches!(m.etat, Etat::Muet))?;
        match m.etat {
            Etat::EnPreparation(depuis) => Some((m.numero, depuis)),
            _ => None,
        }
    }

    /// Fait avancer la tête : les morceaux muets se terminent (les paroles finies, dans l'ordre) ; puis, si la tête est
    /// prête, son son, son numéro et sa parole (elle passe « en cours »).
    fn avancer(&mut self) -> (Vec<u64>, Option<(u64, u64, A)>) {
        let mut finies = Vec::new();
        while let Some(m) = self.morceaux.front_mut() {
            match m.etat {
                Etat::Muet => {
                    if m.dernier {
                        finies.push(m.id);
                    }
                    self.morceaux.pop_front();
                }
                Etat::Pret(_) => {
                    let Etat::Pret(son) = std::mem::replace(&mut m.etat, Etat::EnCours) else { break };
                    return (finies, Some((m.numero, m.id, son)));
                }
                _ => break,
            }
        }
        (finies, None)
    }

    /// Le texte du morceau qui se dit (la tête, « en cours ») ; rien s'il n'y en a pas.
    fn texte_en_cours(&self) -> &str {
        self.morceaux.front().filter(|m| matches!(m.etat, Etat::EnCours)).map_or("", |m| m.texte.as_str())
    }

    /// Le son qui se dira juste après celui en cours, s'il est déjà prêt : il peut s'ouvrir d'avance.
    fn prochain_pret(&self) -> Option<(u64, &A)> {
        let mut suite = self.morceaux.iter();
        if !matches!(suite.next()?.etat, Etat::EnCours) {
            return None;
        }
        match suite.find(|m| !matches!(m.etat, Etat::Muet)) {
            Some(Morceau { numero, etat: Etat::Pret(son), .. }) => Some((*numero, son)),
            _ => None,
        }
    }

    /// Le morceau en cours est fini (dit, ou en échec) : sa parole, et si c'en était le dernier.
    fn lecture_finie(&mut self) -> Option<(u64, bool)> {
        if !matches!(self.morceaux.front()?.etat, Etat::EnCours) {
            return None;
        }
        self.morceaux.pop_front().map(|m| (m.id, m.dernier))
    }

    /// Tout est coupé : les paroles qui restaient, dans l'ordre.
    fn vider(&mut self) -> Vec<u64> {
        self.couper(u64::MAX);
        self.avancer().0
    }
}

// ---- Piper -------------------------------------------------------------------------------------------------------

/// Piper, gardé ouvert : une ligne JSON par phrase (`{"text", "output_file"}`), une ligne par WAV écrit, dans l'ordre
/// des phrases.
struct Piper {
    enfant: Child,
    /// Les lignes JSON pour Piper, écrites par un fil : un tuyau plein (Piper ne lit la phrase suivante qu'après avoir
    /// fini la sienne) n'arrête jamais la voix.
    entree: Sender<String>,
    /// Les WAV écrits, lus par un fil sur la sortie de Piper ; déconnecté : Piper s'est arrêté.
    sorties: Receiver<String>,
    /// Le modèle est chargé : « Initialized piper » sur sa sortie d'erreur, ou un premier WAV.
    pret: Arc<AtomicBool>,
    /// La dernière erreur de Piper (sa sortie d'erreur, ou son entrée fermée).
    erreur: Arc<Mutex<String>>,
    fil_erreurs: Option<thread::JoinHandle<()>>,
    /// Le modèle et la locutrice.
    voix: (String, Option<u32>),
    lance: Instant,
    pret_dit: bool,
    /// Les phrases envoyées dont le WAV n'est pas encore annoncé, dans l'ordre : le morceau et son fichier.
    attendus: VecDeque<(u64, PathBuf)>,
    derniere_sortie: Option<Instant>,
}

impl Piper {
    fn lancer(tige: &str, locuteur: Option<u32>, debit: f64, douceur: Douceur, dossier: &Path) -> io::Result<Piper> {
        let mut c = Command::new(dossier_piper().join("piper.exe"));
        // Les WAV s'écrivent sous un nom relatif, dans le dossier courant de Piper : un chemin accentué (un profil
        // Windows « Hélène ») ne passerait pas par le JSON, que Piper relit dans la page de code du poste.
        c.current_dir(dossier)
            .arg("--model")
            .arg(dossier_voix().join(format!("{tige}.onnx")))
            .arg("--json-input")
            .arg("--length_scale")
            .arg(format!("{:.3}", 1.0 / debit.clamp(0.5, 2.0)))
            .arg("--noise_scale")
            .arg(format!("{:.3}", douceur.bruit))
            .arg("--noise_w")
            .arg(format!("{:.3}", douceur.rythme))
            .arg("--sentence_silence")
            .arg(format!("{:.2}", douceur.silence));
        if let Some(n) = locuteur {
            c.arg("--speaker").arg(n.to_string());
        }
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        crate::cerveau::sans_fenetre(&mut c);
        let mut enfant = c.spawn()?;
        crate::groupe::rattacher(&enfant);
        let (Some(mut stdin), Some(stdout), Some(stderr)) =
            (enfant.stdin.take(), enfant.stdout.take(), enfant.stderr.take())
        else {
            let _ = enfant.kill();
            return Err(io::Error::other("les tuyaux de Piper manquent"));
        };
        let pret = Arc::new(AtomicBool::new(false));
        let erreur = Arc::new(Mutex::new(String::new()));

        let (entree, lignes) = mpsc::channel::<String>();
        let e = erreur.clone();
        thread::spawn(move || {
            for l in lignes {
                if let Err(err) = stdin.write_all(l.as_bytes()).and_then(|()| stdin.flush()) {
                    if let Ok(mut m) = e.lock() {
                        *m = format!("son entrée est fermée ({err})");
                    }
                    break;
                }
            }
        });

        let (tx, sorties) = mpsc::channel();
        thread::spawn(move || {
            let mut r = BufReader::new(stdout);
            let mut ligne = Vec::new();
            loop {
                ligne.clear();
                if r.read_until(b'\n', &mut ligne).unwrap_or(0) == 0 {
                    break;
                }
                let l = String::from_utf8_lossy(&ligne).trim().to_string();
                if !l.is_empty() && tx.send(l).is_err() {
                    break;
                }
            }
        });

        let (p, e) = (pret.clone(), erreur.clone());
        let fil_erreurs = thread::spawn(move || {
            let mut r = BufReader::new(stderr);
            let mut ligne = Vec::new();
            loop {
                ligne.clear();
                if r.read_until(b'\n', &mut ligne).unwrap_or(0) == 0 {
                    break;
                }
                let l = String::from_utf8_lossy(&ligne);
                let l = l.trim();
                if l.contains("Initialized piper") {
                    p.store(true, Ordering::SeqCst);
                } else if !l.is_empty()
                    && (!l.starts_with('[') || ["[error]", "[critical]", "[warning]"].iter().any(|n| l.contains(n)))
                {
                    // Ses erreurs seulement : Piper écrit une ligne d'information par phrase.
                    journal::ecrire(&format!("piper : {l}"));
                    if let Ok(mut m) = e.lock() {
                        *m = l.to_string();
                    }
                }
            }
        });

        Ok(Piper {
            enfant,
            entree,
            sorties,
            pret,
            erreur,
            fil_erreurs: Some(fil_erreurs),
            voix: (tige.to_string(), locuteur),
            lance: Instant::now(),
            pret_dit: false,
            attendus: VecDeque::new(),
            derniere_sortie: None,
        })
    }

    /// Une phrase pour Piper, vers le fichier `nom` de `dossier`.
    fn envoyer(&mut self, numero: u64, texte: &str, nom: &str, dossier: &Path) -> Result<(), String> {
        // Une ligne par phrase : les retours à la ligne du texte n'ont rien à faire dans le JSON de Piper.
        let texte = texte.split_whitespace().collect::<Vec<_>>().join(" ");
        let ligne = format!("{}\n", json!({ "text": texte, "output_file": nom }));
        self.entree.send(ligne).map_err(|_| "Piper ne lit plus ses phrases".to_string())?;
        self.attendus.push_back((numero, dossier.join(nom)));
        Ok(())
    }

    fn parle(&self, numero: u64) -> bool {
        self.attendus.iter().any(|(n, _)| *n == numero)
    }

    /// La dernière erreur de Piper, à la suite d'un message.
    fn detail(&self) -> String {
        match self.erreur.lock().map(|e| e.clone()) {
            Ok(e) if !e.is_empty() => format!(" ({e})"),
            _ => String::new(),
        }
    }

    /// Arrête Piper s'il tourne encore, et attend que sa sortie d'erreur soit lue jusqu'au bout.
    fn finir(&mut self) {
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
        if let Some(f) = self.fil_erreurs.take() {
            let _ = f.join();
        }
    }
}

impl Drop for Piper {
    fn drop(&mut self) {
        self.finir();
    }
}

/// Un WAV annoncé par Piper, à lire dès qu'il est complet.
struct ALire {
    numero: u64,
    chemin: PathBuf,
    depuis: Instant,
}

/// La voix qui parle : Piper (son modèle et sa locutrice) ou une voix de Windows (son nom).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Moteur {
    Piper(String, Option<u32>),
    Windows(String),
    Aucun,
}

fn moteur(choisie: &str) -> Moteur {
    if let Some(reste) = choisie.strip_prefix("piper:") {
        match reste.split_once('#') {
            Some((tige, n)) => Moteur::Piper(tige.to_string(), n.parse().ok()),
            None => Moteur::Piper(reste.to_string(), None),
        }
    } else if let Some(nom) = choisie.strip_prefix("windows:") {
        Moteur::Windows(nom.to_string())
    } else {
        Moteur::Aucun
    }
}

/// Ce que deviennent les phrases que préparait un Piper qu'on arrête.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Suite {
    /// Refaites par la voix choisie (une coupure, une autre voix, le repli).
    Refaire,
    /// Abandonnées : leurs paroles se terminent à leur tour (une panne).
    Abandonner,
}

/// Une panne de Piper.
enum Panne {
    /// Il ne démarre pas, ou s'arrête avant d'avoir chargé son modèle : le repli, tout de suite.
    Demarrage(String),
    /// Il s'est arrêté, ou ne répond plus.
    Arret(String),
    /// Un de ses WAV ne se lit pas.
    Illisible(String),
}

/// Le morceau qui se dit : sa parole, s'il s'entend déjà (l'horloge du lecteur a commencé), et l'instant où son son
/// commence — estimé quand le lecteur le reçoit, puis lu sur son horloge ([`lecture::Lecteur::debut`]).
struct EnLecture {
    id: u64,
    entendu: bool,
    debut: Instant,
}

/// Ce que dit le lecteur du son en cours.
enum Lu {
    EnCours,
    /// Il ne reste que son silence final (ce temps) : le son suivant peut partir.
    Silence(Duration),
    Fini,
    Echec(String),
}

/// Une synthèse de Windows en cours : sondée à chaque tour, sans l'attendre.
#[cfg_attr(not(windows), allow(dead_code))]
struct Synthese {
    depuis: Instant,
    sonder: Box<dyn FnMut() -> Option<Result<Vec<u8>, String>>>,
    annuler: Box<dyn FnOnce()>,
}

#[cfg_attr(not(windows), allow(dead_code))]
impl Synthese {
    fn nouvelle(
        sonder: impl FnMut() -> Option<Result<Vec<u8>, String>> + 'static,
        annuler: impl FnOnce() + 'static,
    ) -> Synthese {
        Synthese { depuis: Instant::now(), sonder: Box::new(sonder), annuler: Box::new(annuler) }
    }

    /// `None` tant qu'elle travaille ; puis le WAV, ou l'erreur.
    fn sonder(&mut self) -> Option<Result<Vec<u8>, String>> {
        (self.sonder)()
    }

    fn annuler(self) {
        (self.annuler)();
    }
}

// ---- Le fil de la voix ---------------------------------------------------------------------------------------------

struct Fil {
    rapport: Sender<Evt>,
    trace: Option<Sender<Trace>>,
    coupure: Arc<Coupure>,
    /// Le seuil de coupure déjà appliqué.
    coupe_vue: u64,
    liste: Vec<VoixDispo>,
    choisie: String,
    debit: f64,
    douceur: Douceur,
    /// Le dossier des WAV de Piper (`parole`).
    dossier: PathBuf,
    file: File<Vec<u8>>,
    piper: Option<Piper>,
    /// Les pannes de Piper à la suite (un WAV bien lu remet le compte à zéro) : à la deuxième, le repli.
    pannes: u32,
    /// Les WAV annoncés par Piper, à lire dès qu'ils sont complets.
    a_lire: VecDeque<ALire>,
    synthetiseur: Synthetiseur,
    /// La synthèse de Windows en cours, et son morceau.
    synthese: Option<(u64, Synthese)>,
    lecteur: Lecteur,
    /// Le morceau qui se dit.
    lecture: Option<EnLecture>,
    /// La dernière parole dont le premier son s'est entendu (`Evt::VoixEntendue`).
    entendue: u64,
    /// Les fichiers à supprimer (Piper peut tenir un WAV ouvert un instant de plus), depuis quand.
    a_supprimer: Vec<(PathBuf, Instant)>,
    fichiers: u64,
    sans_voix_dit: bool,
    erreur_lecture_dite: bool,
    erreur_windows_dite: bool,
}

impl Fil {
    fn ouvrir(
        voulue: &str,
        debit: f64,
        douceur: Douceur,
        coupure: Arc<Coupure>,
        rapport: Sender<Evt>,
        trace: Option<Sender<Trace>>,
    ) -> Fil {
        initialiser_winrt();
        let dossier = dossier_local().join("parole");
        let _ = fs::create_dir_all(&dossier);
        vider_parole(&dossier);
        let liste = lister();
        let choisie = choisir_dans(&liste, voulue);
        let _ = rapport.send(Evt::Voix { liste: liste.clone(), choisie: choisie.clone() });
        let mut fil = Fil {
            rapport,
            trace,
            coupure,
            coupe_vue: 0,
            liste,
            choisie,
            debit,
            douceur,
            dossier,
            file: File::nouvelle(),
            piper: None,
            pannes: 0,
            a_lire: VecDeque::new(),
            synthetiseur: Synthetiseur::nouveau(),
            synthese: None,
            lecteur: Lecteur::nouveau(),
            lecture: None,
            entendue: 0,
            a_supprimer: Vec::new(),
            fichiers: 0,
            sans_voix_dit: false,
            erreur_lecture_dite: false,
            erreur_windows_dite: false,
        };
        // Préchauffé : le modèle de Piper se charge pendant que rien ne se dit encore — et pendant que les lecteurs de
        // Windows s'ouvrent.
        if let Moteur::Piper(tige, locuteur) = moteur(&fil.choisie) {
            fil.lancer_piper(&tige, locuteur);
        }
        fil.lecteur.prechauffer();
        fil
    }

    /// La boucle : les ordres sans attendre, puis un tour de la file ; jusqu'à ce que la `Voix` disparaisse.
    fn mener(mut self, rx: &Receiver<Ordre>) {
        loop {
            match rx.recv_timeout(if self.occupe() { PAS } else { PAS_AU_REPOS }) {
                Ok(o) => {
                    self.ordre(o);
                    while let Ok(o) = rx.try_recv() {
                        self.ordre(o);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            self.tour();
        }
        self.fermer();
    }

    fn occupe(&self) -> bool {
        !self.file.vide()
            || self.lecture.is_some()
            || !self.a_lire.is_empty()
            || !self.a_supprimer.is_empty()
            || self.piper.as_ref().is_some_and(|p| !p.pret_dit)
    }

    fn ordre(&mut self, o: Ordre) {
        match o {
            Ordre::Dire { id, texte } => {
                // Coupée avant d'arriver jusqu'ici : elle se termine à son tour, sans un mot.
                let coupee = id <= self.coupure.seuil();
                self.file.ajouter(id, &texte, coupee);
            }
            Ordre::Choisir(id) => self.choisir(id),
            Ordre::Taire => {}
        }
    }

    fn tour(&mut self) {
        self.couper();
        self.ecouter_piper();
        if let Some(e) = self.lire_wavs(false) {
            self.panne(Panne::Illisible(e));
        }
        self.preparer();
        self.surveiller();
        self.jouer();
        self.supprimer();
    }

    fn tracer(&self, t: Trace) {
        if let Some(s) = &self.trace {
            let _ = s.send(t);
        }
    }

    fn finir(&self, id: u64) {
        let _ = self.rapport.send(Evt::VoixFinie(id));
    }

    /// Le son a fini, ou s'est tu : la bouche du portrait se ferme.
    fn plus_rien_ne_s_entend(&self) {
        faire_entendre(None);
    }

    /// La coupure : le son s'arrête aussitôt, tout ce qui a été donné jusqu'au seuil se termine sans un mot, et Piper,
    /// s'il prépare encore des phrases coupées, est relancé plutôt qu'attendu (le processeur va à l'oreille).
    fn couper(&mut self) {
        let seuil = self.coupure.seuil();
        if seuil <= self.coupe_vue {
            return;
        }
        self.coupe_vue = seuil;
        if self.file.couper(seuil) {
            self.lecteur.arreter();
            self.lecture = None;
            self.plus_rien_ne_s_entend();
            self.tracer(Trace::Coupee(Instant::now()));
        }
        if self.synthese.as_ref().is_some_and(|(n, _)| !self.file.vivant(*n))
            && let Some((_, s)) = self.synthese.take()
        {
            s.annuler();
        }
        let coupees =
            self.piper.as_ref().map_or(0, |p| p.attendus.iter().filter(|(n, _)| !self.file.vivant(*n)).count());
        if coupees > 0 {
            journal::ecrire(&format!(
                "voix : coupée ; Piper relancé plutôt que d'attendre {coupees} phrase(s) coupée(s)"
            ));
            self.relancer_piper();
        }
    }

    fn lancer_piper(&mut self, tige: &str, locuteur: Option<u32>) {
        match Piper::lancer(tige, locuteur, self.debit, self.douceur, &self.dossier) {
            Ok(p) => {
                self.piper = Some(p);
                self.tracer(Trace::PiperLance(Instant::now()));
            }
            Err(e) => self.panne(Panne::Demarrage(format!("Piper ne démarre pas ({e})"))),
        }
    }

    /// Arrête Piper et le relance aussitôt, préchauffé : ce qu'il préparait encore pour des phrases vivantes est refait.
    fn relancer_piper(&mut self) {
        let Some(p) = self.piper.take() else { return };
        let (tige, locuteur) = p.voix.clone();
        self.arreter_piper(p, Suite::Refaire);
        self.tracer(Trace::PiperRelance(Instant::now()));
        self.lancer_piper(&tige, locuteur);
    }

    /// Arrête Piper : les WAV qu'il a annoncés se lisent tels quels ; ce qu'il préparait encore est refait ou abandonné.
    fn arreter_piper(&mut self, mut p: Piper, suite: Suite) {
        p.finir();
        let attendus = std::mem::take(&mut p.attendus);
        drop(p);
        let _ = self.lire_wavs(true);
        let maintenant = Instant::now();
        for (numero, chemin) in attendus {
            self.a_supprimer.push((chemin, maintenant));
            match suite {
                Suite::Refaire => self.file.reprendre(numero),
                Suite::Abandonner => self.file.abandonner(numero),
            }
        }
    }

    /// Ce que Piper a écrit sur sa sortie : un WAV par phrase, dans l'ordre ; la sortie fermée, il s'est arrêté.
    fn ecouter_piper(&mut self) {
        let maintenant = Instant::now();
        let Some(p) = self.piper.as_mut() else { return };
        let mut arrete = false;
        loop {
            match p.sorties.try_recv() {
                Ok(ligne) => {
                    p.derniere_sortie = Some(maintenant);
                    p.pret.store(true, Ordering::SeqCst);
                    let chemin = PathBuf::from(&ligne);
                    let chemin = if chemin.is_absolute() { chemin } else { self.dossier.join(chemin) };
                    match p.attendus.pop_front() {
                        Some((numero, attendu)) => {
                            if attendu != chemin {
                                self.a_supprimer.push((attendu, maintenant));
                            }
                            self.a_lire.push_back(ALire { numero, chemin, depuis: maintenant });
                        }
                        None => self.a_supprimer.push((chemin, maintenant)),
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    arrete = true;
                    break;
                }
            }
        }
        if arrete {
            // Sa sortie d'erreur lue jusqu'au bout : avait-il chargé son modèle, qu'a-t-il dit en dernier ?
            p.finir();
            let pret = p.pret.load(Ordering::SeqCst);
            let detail = p.detail();
            self.panne(if pret {
                Panne::Arret(format!("Piper s'est arrêté{detail}"))
            } else {
                Panne::Demarrage(format!("Piper ne démarre pas{detail}"))
            });
            return;
        }
        if !p.pret_dit && p.pret.load(Ordering::SeqCst) {
            p.pret_dit = true;
            journal::ecrire(&format!("voix : Piper prêt en {:.1} s", p.lance.elapsed().as_secs_f32()));
            self.tracer(Trace::PiperPret(Instant::now()));
        }
    }

    /// Lit les WAV annoncés par Piper dès qu'ils sont complets ; `fin` : Piper est arrêté, ce qui est là se lit tel
    /// quel. Rend l'erreur d'un WAV illisible.
    fn lire_wavs(&mut self, fin: bool) -> Option<String> {
        let maintenant = Instant::now();
        let mut erreur = None;
        for a in std::mem::take(&mut self.a_lire) {
            if !self.file.vivant(a.numero) {
                self.a_supprimer.push((a.chemin, maintenant));
                continue;
            }
            match lire_wav(&a.chemin, fin || maintenant.duration_since(a.depuis) > DELAI_WAV) {
                Ok(None) => self.a_lire.push_back(a),
                Ok(Some(mut wav)) => {
                    adoucir(&mut wav, self.douceur);
                    self.file.pret(a.numero, wav);
                    self.pannes = 0;
                    self.a_supprimer.push((a.chemin, maintenant));
                }
                Err(e) => {
                    journal::ecrire(&format!("voix : WAV de Piper illisible ({}) : {e}", a.chemin.display()));
                    self.file.abandonner(a.numero);
                    self.a_supprimer.push((a.chemin, maintenant));
                    erreur = Some(format!("Piper a rendu un son illisible ({e})"));
                }
            }
        }
        erreur
    }

    /// Une panne de Piper. Arrêté, il est relancé à la phrase suivante ; mais s'il ne démarre pas, ou au deuxième échec
    /// de suite, une voix de Windows prend la suite pour la séance.
    fn panne(&mut self, panne: Panne) {
        self.pannes += 1;
        let (message, repli, arreter) = match panne {
            Panne::Demarrage(m) => (m, true, true),
            Panne::Arret(m) => (m, self.pannes >= 2, true),
            // Un WAV illisible ne perd que sa phrase : Piper continue.
            Panne::Illisible(m) => (m, self.pannes >= 2, false),
        };
        journal::ecrire(&format!("voix : {message}"));
        if repli {
            self.repli(&message);
            return;
        }
        if arreter && let Some(p) = self.piper.take() {
            self.arreter_piper(p, Suite::Abandonner);
        }
        let _ = self.rapport.send(Evt::VoixErreur(message));
    }

    /// Le repli : la première voix française de femme de Windows (à défaut, une voix française de Windows), dit une
    /// fois ; ce que Piper préparait encore est refait par elle.
    fn repli(&mut self, pourquoi: &str) {
        if let Some(p) = self.piper.take() {
            self.arreter_piper(p, Suite::Refaire);
        }
        let (choisie, message) = match voix_de_repli(&self.liste) {
            Some(v) => (v.id.clone(), format!("{pourquoi} ; je prends {} pour la séance", v.nom)),
            None => {
                self.sans_voix_dit = true;
                (String::new(), format!("{pourquoi}, et ce poste n'a aucune voix française de Windows"))
            }
        };
        journal::ecrire(&format!("voix : repli — {message}"));
        self.choisie = choisie;
        let _ = self.rapport.send(Evt::VoixErreur(message));
        let _ = self.rapport.send(Evt::Voix { liste: self.liste.clone(), choisie: self.choisie.clone() });
    }

    /// Une autre voix : ce que l'ancienne préparait encore est refait par la nouvelle ; une voix de Piper se préchauffe.
    /// Choisir Piper après un repli le réessaie.
    fn choisir(&mut self, id: String) {
        if id != self.choisie && self.liste.iter().any(|v| v.id == id) {
            self.choisie = id;
            self.pannes = 0;
            if let Some((numero, s)) = self.synthese.take() {
                s.annuler();
                self.file.reprendre(numero);
            }
            let voulu = moteur(&self.choisie);
            let garder = self.piper.as_ref().is_some_and(|p| voulu == Moteur::Piper(p.voix.0.clone(), p.voix.1));
            if !garder && let Some(p) = self.piper.take() {
                self.arreter_piper(p, Suite::Refaire);
            }
            if self.piper.is_none()
                && let Moteur::Piper(tige, locuteur) = voulu
            {
                self.lancer_piper(&tige, locuteur);
            }
            journal::ecrire(&format!("voix : {}", self.choisie));
        }
        let _ = self.rapport.send(Evt::Voix { liste: self.liste.clone(), choisie: self.choisie.clone() });
    }

    /// Prépare ce qui est à faire, par la voix choisie : à Piper, tout de suite et dans l'ordre ; à Windows, une phrase
    /// à la fois (elle va bien plus vite que la lecture).
    fn preparer(&mut self) {
        match moteur(&self.choisie) {
            Moteur::Piper(tige, locuteur) => self.preparer_piper(&tige, locuteur),
            Moteur::Windows(nom) => self.preparer_windows(&nom),
            Moteur::Aucun => {
                let a_faire = self.file.a_faire();
                if a_faire.is_empty() {
                    return;
                }
                for (numero, _) in a_faire {
                    self.file.abandonner(numero);
                }
                if !self.sans_voix_dit {
                    self.sans_voix_dit = true;
                    let _ = self.rapport.send(Evt::VoixErreur("aucune voix sur ce poste".into()));
                }
            }
        }
    }

    fn preparer_piper(&mut self, tige: &str, locuteur: Option<u32>) {
        if self.file.premier_a_faire().is_none() {
            return;
        }
        // Relancé à la phrase suivante après une panne (ou une autre voix de Piper).
        if self.piper.as_ref().is_none_or(|p| p.voix.0 != tige || p.voix.1 != locuteur) {
            if let Some(p) = self.piper.take() {
                self.arreter_piper(p, Suite::Refaire);
            }
            self.lancer_piper(tige, locuteur);
        }
        let maintenant = Instant::now();
        for (numero, texte) in self.file.a_faire() {
            self.fichiers += 1;
            let nom = format!("phrase-{}-{}.wav", std::process::id(), self.fichiers);
            // Sans Piper (le repli vient d'avoir lieu), la voix de Windows reprend au tour suivant.
            let Some(p) = self.piper.as_mut() else { return };
            if let Err(e) = p.envoyer(numero, &texte, &nom, &self.dossier) {
                let detail = p.detail();
                self.panne(Panne::Arret(format!("{e}{detail}")));
                return;
            }
            self.file.en_preparation(numero, maintenant);
        }
    }

    fn preparer_windows(&mut self, nom: &str) {
        let maintenant = Instant::now();
        if let Some((numero, s)) = self.synthese.as_mut() {
            let numero = *numero;
            let resultat = match s.sonder() {
                Some(r) => Some(r),
                None if maintenant.duration_since(s.depuis) > DELAI_WINDOWS => None,
                None => return,
            };
            let finie = self.synthese.take();
            if resultat.is_none()
                && let Some((_, s)) = finie
            {
                s.annuler();
            }
            match resultat {
                Some(Ok(wav)) => {
                    self.file.pret(numero, wav);
                    self.erreur_windows_dite = false;
                }
                Some(Err(e)) => self.echec_windows(numero, e),
                None => self.echec_windows(numero, "la voix de Windows ne répond plus".into()),
            }
        }
        if self.synthese.is_none()
            && let Some((numero, texte)) = self.file.premier_a_faire()
        {
            match self.synthetiseur.dire_windows(nom, &texte, self.debit) {
                Ok(s) => {
                    self.file.en_preparation(numero, maintenant);
                    self.synthese = Some((numero, s));
                }
                Err(e) => self.echec_windows(numero, e),
            }
        }
    }

    fn echec_windows(&mut self, numero: u64, e: String) {
        self.file.abandonner(numero);
        if !self.erreur_windows_dite {
            self.erreur_windows_dite = true;
            journal::ecrire(&format!("voix : la synthèse de Windows a échoué : {e}"));
            let _ = self.rapport.send(Evt::VoixErreur(e));
        }
    }

    /// La phrase en tête attend son WAV de Piper depuis trop longtemps (depuis son envoi, ou depuis le dernier WAV que
    /// Piper a rendu) : Piper ne répond plus.
    fn surveiller(&mut self) {
        let Some((numero, depuis)) = self.file.tete_en_preparation() else { return };
        let Some(p) = &self.piper else { return };
        if !p.parle(numero) {
            return;
        }
        let reference = p.derniere_sortie.map_or(depuis, |d| d.max(depuis));
        if reference.elapsed() > DELAI_PIPER {
            self.panne(Panne::Arret("Piper ne répond plus".into()));
        }
    }

    /// Le son en cours fini, le suivant part aussitôt ; les paroles se terminent à mesure que la tête avance.
    fn jouer(&mut self) {
        if let Some(l) = self.lecture.as_mut() {
            let lu = self.lecteur.sonder();
            // Le début du son, lu sur l'horloge du lecteur : la bouche s'y recale (elle partait de l'estimation).
            let debut = self.lecteur.debut();
            if let Some(d) = debut
                && d != l.debut
            {
                l.debut = d;
                recaler(d);
            }
            let entendu = !l.entendu && debut.is_some();
            l.entendu |= entendu;
            let (id, depuis) = (l.id, l.debut);
            if entendu {
                self.tracer(Trace::Son { id, a: depuis });
                // Le premier son de cette parole s'entend : le chef en tient le temps de la première parole.
                if id > self.entendue {
                    self.entendue = id;
                    let _ = self.rapport.send(Evt::VoixEntendue(id));
                }
            }
            let maintenant = Instant::now();
            // La fin du son : maintenant, ou la fin de son silence final quand le suivant part pendant ce silence.
            let fin = match lu {
                Lu::EnCours => {
                    // Le son qui suit s'ouvre déjà dans l'autre lecteur : il partira sans attendre.
                    if let Some((numero, wav)) = self.file.prochain_pret() {
                        self.lecteur.precharger(numero, wav);
                    }
                    return;
                }
                // Rien de prêt à enchaîner : celui-ci finit son silence.
                Lu::Silence(_) if self.file.prochain_pret().is_none() => return,
                Lu::Silence(reste) => maintenant + reste,
                Lu::Fini => {
                    self.erreur_lecture_dite = false;
                    maintenant
                }
                Lu::Echec(e) => {
                    self.echec_lecture(e);
                    maintenant
                }
            };
            self.lecture = None;
            self.plus_rien_ne_s_entend();
            if let Some((id, dernier)) = self.file.lecture_finie() {
                self.tracer(Trace::Fin { id, a: fin });
                if dernier {
                    self.finir(id);
                }
            }
        }
        loop {
            let (finies, suivant) = self.file.avancer();
            for id in finies {
                self.finir(id);
            }
            let Some((numero, id, wav)) = suivant else { return };
            let piste = piste_du_son(&wav, self.file.texte_en_cours());
            match self.lecteur.jouer(numero, wav) {
                Ok(attente) => {
                    // Le son partira dans un instant : la bouche le sait d'avance — l'écran montre ses images un peu
                    // après les avoir calculées ([`regler_le_retard_de_l_ecran`]), et le début de la phrase s'y voit.
                    let debut = Instant::now() + attente;
                    entendre(Some((debut, piste)));
                    self.lecture = Some(EnLecture { id, entendu: false, debut });
                    self.tracer(Trace::Debut { id, a: Instant::now() });
                    return;
                }
                Err(e) => {
                    self.echec_lecture(e);
                    if let Some((id, true)) = self.file.lecture_finie() {
                        self.finir(id);
                    }
                }
            }
        }
    }

    fn echec_lecture(&mut self, e: String) {
        if !self.erreur_lecture_dite {
            self.erreur_lecture_dite = true;
            journal::ecrire(&format!("voix : le son ne se joue pas : {e}"));
            let _ = self.rapport.send(Evt::VoixErreur(format!("le son ne se joue pas ({e})")));
        }
    }

    fn supprimer(&mut self) {
        let maintenant = Instant::now();
        self.a_supprimer.retain(|(chemin, depuis)| match fs::remove_file(chemin) {
            Ok(()) => false,
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(_) => maintenant.duration_since(*depuis) < Duration::from_secs(10),
        });
    }

    /// La `Voix` a disparu : tout se tait, Piper s'arrête, et chaque parole qui restait se termine.
    fn fermer(&mut self) {
        self.lecteur.arreter();
        self.lecture = None;
        self.plus_rien_ne_s_entend();
        if let Some((_, s)) = self.synthese.take() {
            s.annuler();
        }
        let restantes = self.file.vider();
        if let Some(p) = self.piper.take() {
            self.arreter_piper(p, Suite::Abandonner);
        }
        for id in restantes {
            self.finir(id);
        }
        self.supprimer();
    }
}

/// Les WAV restés d'une séance précédente.
fn vider_parole(dossier: &Path) {
    let Ok(d) = fs::read_dir(dossier) else { return };
    for e in d.flatten() {
        let nom = e.file_name().to_string_lossy().into_owned();
        let vieux =
            e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a > AGE_RESTE);
        if nom.starts_with("phrase-") && nom.ends_with(".wav") && vieux {
            let _ = fs::remove_file(e.path());
        }
    }
}

// ---- Windows --------------------------------------------------------------------------------------------------------

#[cfg(windows)]
pub fn initialiser_winrt() {
    // SAFETY : l'initialisation de WinRT pour le fil courant, avant tout objet WinRT ; un second appel est sans effet.
    let _ =
        unsafe { windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED) };
}

#[cfg(not(windows))]
pub fn initialiser_winrt() {}

#[cfg(windows)]
fn voix_windows() -> Vec<VoixDispo> {
    use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceGender};
    let Ok(toutes) = SpeechSynthesizer::AllVoices() else { return Vec::new() };
    (0..toutes.Size().unwrap_or(0))
        .filter_map(|i| toutes.GetAt(i).ok())
        .filter(|v| v.Language().map(|l| l.to_string().starts_with("fr")).unwrap_or(false))
        .filter_map(|v| {
            let nom = v.DisplayName().ok()?.to_string();
            let femme = v.Gender().map(|g| g == VoiceGender::Female).unwrap_or(false);
            let court = nom.trim_start_matches("Microsoft ").to_string();
            Some(VoixDispo { id: format!("windows:{nom}"), nom: libelle(&court, "Windows", femme), femme })
        })
        .collect()
}

#[cfg(not(windows))]
fn voix_windows() -> Vec<VoixDispo> {
    Vec::new()
}

/// Le lecteur de Windows, qui joue un WAV en mémoire sans qu'on l'attende, et la synthèse des voix de Windows.
#[cfg(windows)]
mod lecture {
    use std::time::{Duration, Instant};

    use windows::Media::Core::MediaSource;
    use windows::Media::Playback::{MediaPlaybackState, MediaPlayer};
    use windows::Media::SpeechSynthesis::{SpeechSynthesisStream, SpeechSynthesizer};
    use windows::Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream};
    use windows::core::HSTRING;

    use super::{AVANCE, DELAI_DEMARRAGE, Lu, Synthese, duree_wav, silence_final};

    fn texte(e: windows::core::Error) -> String {
        e.to_string()
    }

    /// Un WAV en mémoire, prêt pour un lecteur.
    fn source(wav: &[u8]) -> Result<MediaSource, String> {
        let flux = InMemoryRandomAccessStream::new().map_err(texte)?;
        let ecrivain = DataWriter::CreateDataWriter(&flux).map_err(texte)?;
        ecrivain.WriteBytes(wav).map_err(texte)?;
        ecrivain.StoreAsync().map_err(texte)?.join().map_err(texte)?;
        // Le flux reste au lecteur quand l'écrivain s'en va.
        ecrivain.DetachStream().map_err(texte)?;
        flux.Seek(0).map_err(texte)?;
        MediaSource::CreateFromStream(&flux, &HSTRING::from("audio/wav")).map_err(texte)
    }

    /// Deux lecteurs de Windows : l'un joue, l'autre ouvre déjà le son qui suit. Ouvrir un son prend de 60 à plus de
    /// 400 ms (mesuré le 2026-10-22, davantage quand Piper occupe le processeur) ; ouvert d'avance, il part au
    /// `Play()`.
    pub struct Lecteur {
        joueurs: Vec<MediaPlayer>,
        actif: usize,
        /// Le morceau donné d'avance à l'autre lecteur, et s'il a pu l'ouvrir.
        precharge: Option<(u64, bool)>,
        depuis: Instant,
        /// Le son en cours a été vu jouer (depuis cet instant) : la pause qui suit est sa fin.
        a_joue: Option<Instant>,
        /// L'instant où le son en cours a commencé, d'après l'horloge du lecteur ([`super::debut_du_son`]).
        debut: Option<Instant>,
        duree: Option<Duration>,
        /// Le silence à la fin du son en cours.
        silence: Duration,
    }

    /// Ce que met le son à partir après `Play()`, mesuré dans le son qui sort du poste (le 2026-10-25) : ouvert
    /// d'avance, 15 ms ; ouvert à l'instant, de 70 à 76 ms. La bouche s'y fie avant que l'horloge du lecteur parle.
    const ATTENTE_OUVERT_D_AVANCE: Duration = Duration::from_millis(15);
    const ATTENTE_OUVERT_A_L_INSTANT: Duration = Duration::from_millis(74);
    /// Un lecteur qui joue sans que sa position avance : le son a commencé quand il a été vu jouer.
    const SANS_POSITION: Duration = Duration::from_millis(100);

    impl Lecteur {
        /// Un lecteur encore fermé : [`Lecteur::prechauffer`] l'ouvre (après le lancement de Piper, dont le modèle se
        /// charge pendant ce temps).
        pub fn nouveau() -> Lecteur {
            Lecteur {
                joueurs: Vec::new(),
                actif: 0,
                precharge: None,
                depuis: Instant::now(),
                a_joue: None,
                debut: None,
                duree: None,
                silence: Duration::ZERO,
            }
        }

        /// Ouvre les deux lecteurs, et leur fait jouer un dixième de seconde de silence : la lecture de Windows se met
        /// en place avant la première phrase (sa toute première ouverture coûte quelques centaines de millisecondes).
        pub fn prechauffer(&mut self) {
            if !self.joueurs.is_empty() {
                return;
            }
            self.joueurs = (0..2).filter_map(|_| MediaPlayer::new().ok()).collect();
            let silence = super::wav_mono(&[0; 2205], 22_050);
            for j in &self.joueurs {
                let _ = j.SetAutoPlay(false);
                if let Ok(s) = source(&silence) {
                    let _ = j.SetSource(&s);
                    let _ = j.Play();
                }
            }
        }

        /// Ouvre d'avance, dans le lecteur libre, le son qui suit celui en cours (une fois par morceau).
        pub fn precharger(&mut self, numero: u64, wav: &[u8]) {
            if self.joueurs.len() < 2 || self.precharge.is_some_and(|(n, _)| n == numero) {
                return;
            }
            let libre = &self.joueurs[(self.actif + 1) % 2];
            let ouvert = source(wav).and_then(|s| libre.SetSource(&s).map_err(texte)).is_ok();
            self.precharge = Some((numero, ouvert));
        }

        /// Joue un WAV sans attendre, dans l'autre lecteur (celui qui vient de jouer peut finir son silence) : ouvert
        /// d'avance s'il l'est déjà. Rend le temps que le son mettra sans doute à partir ; [`Lecteur::debut`] dit
        /// quand il est vraiment parti, [`Lecteur::sonder`] quand il est fini.
        pub fn jouer(&mut self, numero: u64, wav: Vec<u8>) -> Result<Duration, String> {
            self.prechauffer();
            if self.joueurs.is_empty() {
                return Err("le lecteur audio de Windows ne s'ouvre pas".into());
            }
            let libre = (self.actif + 1) % self.joueurs.len();
            let ouvert = self.precharge.take() == Some((numero, true));
            if !ouvert {
                let s = source(&wav)?;
                self.joueurs[libre].SetSource(&s).map_err(texte)?;
            }
            self.actif = libre;
            self.joueurs[self.actif].Play().map_err(texte)?;
            self.depuis = Instant::now();
            self.a_joue = None;
            self.debut = None;
            self.duree = duree_wav(&wav);
            self.silence = silence_final(&wav);
            Ok(if ouvert { ATTENTE_OUVERT_D_AVANCE } else { ATTENTE_OUVERT_A_L_INSTANT })
        }

        pub fn sonder(&mut self) -> Lu {
            let Some(j) = self.joueurs.get(self.actif) else {
                return Lu::Echec("le lecteur audio de Windows ne s'ouvre pas".into());
            };
            let session = match j.PlaybackSession() {
                Ok(s) => s,
                Err(e) => return Lu::Echec(e.to_string()),
            };
            let etat = match session.PlaybackState() {
                Ok(e) => e,
                Err(e) => return Lu::Echec(e.to_string()),
            };
            let ecoule = self.depuis.elapsed();
            if etat == MediaPlaybackState::Playing {
                self.a_joue.get_or_insert_with(Instant::now);
                if let Ok(p) = session.Position().map(Duration::from) {
                    self.debut = super::debut_du_son(Instant::now(), self.depuis.elapsed(), p, self.debut);
                    // Dans son silence final, assez près de la fin : le son suivant peut partir.
                    if let Some(d) = self.duree {
                        let reste = d.saturating_sub(p);
                        if reste <= AVANCE.min(self.silence) {
                            return Lu::Silence(reste);
                        }
                    }
                }
            } else if self.a_joue.is_some() && (etat == MediaPlaybackState::Paused || etat == MediaPlaybackState::None)
            {
                return Lu::Fini;
            }
            if self.a_joue.is_none() && ecoule > DELAI_DEMARRAGE {
                return Lu::Echec("le son ne démarre pas".into());
            }
            // Un lecteur qui ne rend jamais la main : le temps du son est passé, c'est fini.
            if self.duree.is_some_and(|d| ecoule > d + DELAI_DEMARRAGE) {
                return Lu::Fini;
            }
            Lu::EnCours
        }

        /// L'instant où le son en cours a commencé de s'entendre, lu sur l'horloge du lecteur ; `None` : pas encore.
        /// Un lecteur dont la position n'avance pas : l'instant où il a été vu jouer.
        pub fn debut(&self) -> Option<Instant> {
            self.debut.or_else(|| self.a_joue.filter(|t| t.elapsed() > SANS_POSITION))
        }

        /// Coupe le son tout de suite (les deux lecteurs : l'ancien peut finir son silence), et oublie celui qui
        /// attendait. La pause seulement : retirer la source démonte la lecture, ce qui prend des dizaines de
        /// millisecondes quand Piper occupe le processeur ; le son suivant la remplace de toute façon.
        pub fn arreter(&mut self) {
            for j in &self.joueurs {
                let _ = j.Pause();
            }
            self.precharge = None;
        }
    }

    /// Le synthétiseur de Windows, gardé pour la voix en cours.
    pub struct Synthetiseur {
        courant: Option<(String, SpeechSynthesizer)>,
    }

    impl Synthetiseur {
        pub fn nouveau() -> Synthetiseur {
            Synthetiseur { courant: None }
        }

        fn pour(&mut self, nom: &str, debit: f64) -> Result<SpeechSynthesizer, String> {
            if let Some((n, s)) = &self.courant
                && n == nom
            {
                return Ok(s.clone());
            }
            let s = SpeechSynthesizer::new().map_err(texte)?;
            let toutes = SpeechSynthesizer::AllVoices().map_err(texte)?;
            for i in 0..toutes.Size().map_err(texte)? {
                let v = toutes.GetAt(i).map_err(texte)?;
                if v.DisplayName().is_ok_and(|n| n == nom) {
                    s.SetVoice(&v).map_err(texte)?;
                    break;
                }
            }
            let _ = s.Options().and_then(|o| o.SetSpeakingRate(debit));
            self.courant = Some((nom.to_string(), s.clone()));
            Ok(s)
        }

        /// Lance la synthèse d'une phrase par une voix de Windows, sans l'attendre : elle se sonde à chaque tour.
        pub fn dire_windows(&mut self, nom: &str, phrase: &str, debit: f64) -> Result<Synthese, String> {
            let op = self.pour(nom, debit)?.SynthesizeTextToStreamAsync(&HSTRING::from(phrase)).map_err(texte)?;
            let a_annuler = op.clone();
            // AsyncStatus (de windows-future, que le projet ne nomme pas) : 0 en cours, 1 finie, 2 annulée, 3 erreur.
            let sonder = move || match op.Status().map(|s| s.0) {
                Ok(0) => None,
                Ok(1) => Some(op.GetResults().map_err(texte).and_then(|f| octets(&f))),
                Ok(_) => Some(Err(op.ErrorCode().map(|c| c.message()).unwrap_or_else(|e| e.message()))),
                Err(e) => Some(Err(e.to_string())),
            };
            Ok(Synthese::nouvelle(sonder, move || {
                let _ = a_annuler.Cancel();
            }))
        }
    }

    /// Le WAV que la synthèse de Windows a écrit dans son flux.
    fn octets(f: &SpeechSynthesisStream) -> Result<Vec<u8>, String> {
        let taille = u32::try_from(f.Size().map_err(texte)?).map_err(|_| "un son trop long".to_string())?;
        let lecteur = DataReader::CreateDataReader(&f.GetInputStreamAt(0).map_err(texte)?).map_err(texte)?;
        let lu = lecteur.LoadAsync(taille).map_err(texte)?.join().map_err(texte)?;
        let mut o = vec![0u8; lu as usize];
        lecteur.ReadBytes(&mut o).map_err(texte)?;
        Ok(o)
    }
}

#[cfg(not(windows))]
mod lecture {
    use std::time::{Duration, Instant};

    use super::{Lu, Synthese};

    pub struct Lecteur;

    impl Lecteur {
        pub fn nouveau() -> Lecteur {
            Lecteur
        }
        pub fn prechauffer(&mut self) {}
        pub fn precharger(&mut self, _numero: u64, _wav: &[u8]) {}
        pub fn jouer(&mut self, _numero: u64, _wav: Vec<u8>) -> Result<Duration, String> {
            Err("la lecture du son n'existe que sous Windows".into())
        }
        pub fn sonder(&mut self) -> Lu {
            Lu::Fini
        }
        pub fn debut(&self) -> Option<Instant> {
            None
        }
        pub fn arreter(&mut self) {}
    }

    pub struct Synthetiseur;

    impl Synthetiseur {
        pub fn nouveau() -> Synthetiseur {
            Synthetiseur
        }
        pub fn dire_windows(&mut self, _nom: &str, _phrase: &str, _debit: f64) -> Result<Synthese, String> {
            Err("les voix de Windows n'existent que sous Windows".into())
        }
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn les_phrases_se_decoupent() {
        let p = decouper(
            "Deux choses pressent aujourd'hui. Appeler Toitures Exemple ; puis valider le plan du fournisseur !",
        );
        assert_eq!(
            p,
            vec![
                "Deux choses pressent aujourd'hui.",
                "Appeler Toitures Exemple ; puis valider le plan du fournisseur !"
            ]
        );
        assert_eq!(decouper("Court. Encore."), vec!["Court. Encore."]);
        assert_eq!(
            decouper("Le montant est de 937.05 dollars avec les taxes."),
            vec!["Le montant est de 937.05 dollars avec les taxes."]
        );
    }

    /// Une file d'essai : le son est un simple nom.
    fn file() -> File<&'static str> {
        File::nouvelle()
    }

    fn numeros(f: &File<&'static str>) -> Vec<u64> {
        f.morceaux.iter().map(|m| m.numero).collect()
    }

    /// Les ordres reçus par le fil, versés dans la file comme il le fait.
    fn verser(rx: &Receiver<Ordre>, f: &mut File<&'static str>, c: &Coupure) {
        for o in rx.try_iter() {
            if let Ordre::Dire { id, texte } = o {
                f.ajouter(id, &texte, id <= c.seuil());
            }
        }
    }

    #[test]
    fn chaque_parole_se_termine_une_fois_et_dans_l_ordre() {
        let mut f = file();
        let t = Instant::now();
        f.ajouter(1, "Deux choses pressent aujourd'hui. Appeler Toitures Exemple pour le gypse.", false);
        f.ajouter(2, "  \n ", false);
        f.ajouter(3, "Puis valider le plan du fournisseur.", false);
        let n = numeros(&f);
        assert_eq!(n.len(), 4);
        assert_eq!(f.a_faire().iter().map(|a| a.0).collect::<Vec<_>>(), vec![n[0], n[1], n[3]]);
        for k in [n[0], n[1], n[3]] {
            f.en_preparation(k, t);
        }
        // Les sons arrivent dans le désordre : rien ne se dit avant la tête, et la parole vide attend son tour.
        assert!(f.pret(n[3], "c"));
        assert!(f.pret(n[1], "b"));
        let (finies, suivant) = f.avancer();
        assert!(finies.is_empty() && suivant.is_none());
        assert!(f.pret(n[0], "a"));
        let (mut dits, mut finies) = (Vec::new(), Vec::new());
        loop {
            let (f2, suivant) = f.avancer();
            finies.extend(f2);
            let Some((_, _, son)) = suivant else { break };
            dits.push(son);
            if let Some((id, true)) = f.lecture_finie() {
                finies.push(id);
            }
        }
        assert_eq!(dits, vec!["a", "b", "c"]);
        assert_eq!(finies, vec![1, 2, 3]);
        assert!(f.vide());
    }

    #[test]
    fn taire_coupe_ce_qui_a_ete_donne_et_rien_apres() {
        let (tx, rx) = mpsc::channel();
        let v = Voix { tx, coupure: Arc::new(Coupure::default()) };
        let mut f = file();
        let t = Instant::now();
        v.dire(1, "Une première réponse, assez longue pour compter.");
        v.dire(2, "Et sa suite, qui attend encore son tour.");
        verser(&rx, &mut f, &v.coupure);
        let n = numeros(&f);
        f.en_preparation(n[0], t);
        f.en_preparation(n[1], t);
        assert!(f.pret(n[0], "un"));
        assert_eq!(f.avancer().1.map(|s| s.2), Some("un"));
        v.taire();
        v.dire(3, "Une nouvelle demande, après la coupure.");
        assert_eq!(v.coupure.seuil(), 2);
        // Le son en cours s'arrête, ce qui attendait se termine dans l'ordre, et le son de 2 arrive trop tard.
        assert!(f.couper(v.coupure.seuil()));
        assert!(!f.pret(n[1], "deux"));
        verser(&rx, &mut f, &v.coupure);
        let (finies, suivant) = f.avancer();
        assert_eq!(finies, vec![1, 2]);
        assert!(suivant.is_none());
        // 3, donné après la coupure, se dit normalement (le booléen d'avant pouvait le perdre, ou laisser repartir 2).
        let (numero, _) = f.premier_a_faire().expect("la parole 3 est à faire");
        f.en_preparation(numero, t);
        assert!(f.pret(numero, "trois"));
        let (finies, suivant) = f.avancer();
        assert!(finies.is_empty());
        assert_eq!(suivant.map(|s| (s.1, s.2)), Some((3, "trois")));
        assert_eq!(f.lecture_finie(), Some((3, true)));
        // Une nouvelle coupure ne touche pas ce qui est fini, et une coupure sans rien de neuf ne change rien.
        v.taire();
        assert_eq!(v.coupure.seuil(), 3);
    }

    #[test]
    fn une_parole_coupee_avant_d_arriver_se_termine_a_son_tour() {
        let (tx, rx) = mpsc::channel();
        let v = Voix { tx, coupure: Arc::new(Coupure::default()) };
        let mut f = file();
        // 4 est donnée puis coupée avant que le fil ne lise les ordres ; 5 vient après la coupure.
        v.dire(4, "Elle a été coupée avant que le fil la lise.");
        v.taire();
        v.dire(5, "Celle-ci vient après la coupure.");
        verser(&rx, &mut f, &v.coupure);
        assert_eq!(f.a_faire().len(), 1);
        assert_eq!(f.avancer().0, vec![4]);
        assert!(f.vivant(f.premier_a_faire().unwrap().0));
    }

    #[test]
    fn un_piper_relance_refait_ce_qu_il_preparait() {
        let mut f = file();
        let t = Instant::now();
        f.ajouter(1, "Une phrase que Piper préparait quand on l'a relancé.", false);
        f.ajouter(2, "Une autre, qu'une panne a fait abandonner.", false);
        let n = numeros(&f);
        f.en_preparation(n[0], t);
        f.en_preparation(n[1], t);
        assert_eq!(f.tete_en_preparation(), Some((n[0], t)));
        f.reprendre(n[0]);
        f.abandonner(n[1]);
        assert_eq!(f.a_faire().iter().map(|a| a.0).collect::<Vec<_>>(), vec![n[0]]);
        assert!(!f.vivant(n[1]));
        assert_eq!(f.tete_en_preparation(), None);
        f.en_preparation(n[0], t);
        assert!(f.pret(n[0], "un"));
        assert!(f.avancer().1.is_some());
        assert_eq!(f.lecture_finie(), Some((1, true)));
        assert_eq!(f.avancer().0, vec![2]);
        assert!(f.vide());
    }

    #[test]
    fn fermer_termine_tout_ce_qui_restait() {
        let mut f = file();
        let t = Instant::now();
        f.ajouter(7, "Une phrase qui se dit quand tout se ferme.", false);
        f.ajouter(8, "Une autre qui attendait.", false);
        let n = numeros(&f);
        f.en_preparation(n[0], t);
        f.pret(n[0], "sept");
        assert!(f.avancer().1.is_some());
        assert_eq!(f.vider(), vec![7, 8]);
        assert!(f.vide());
    }

    #[test]
    fn le_son_qui_suit_s_ouvre_d_avance_s_il_est_pret() {
        let mut f = file();
        let t = Instant::now();
        f.ajouter(1, "La phrase qui se dit en ce moment même.", false);
        f.ajouter(2, "", false);
        f.ajouter(3, "Celle qui suit, déjà prête.", false);
        f.ajouter(4, "Puis une autre, encore chez Piper.", false);
        let n = numeros(&f);
        for k in [n[0], n[2], n[3]] {
            f.en_preparation(k, t);
        }
        f.pret(n[0], "un");
        assert!(f.prochain_pret().is_none(), "rien ne se dit encore");
        assert!(f.avancer().1.is_some());
        assert!(f.prochain_pret().is_none(), "la suivante n'est pas prête");
        f.pret(n[3], "quatre");
        assert!(f.prochain_pret().is_none(), "la parole 3 passe avant la 4");
        f.pret(n[2], "trois");
        // La parole vide (2) se terminera à son tour : la suivante à se dire est la 3.
        assert_eq!(f.prochain_pret(), Some((n[2], &"trois")));
    }

    /// Un WAV comme ceux de Piper : 22 050 Hz, 16 bits, mono.
    fn wav(echantillons: &[i16]) -> Vec<u8> {
        wav_mono(echantillons, 22_050)
    }

    #[test]
    fn un_wav_annonce_trop_tot_se_relit() {
        let w = wav(&vec![1000; 22_050]);
        assert!(wav_complet(&w));
        assert_eq!(duree_wav(&w), Some(Duration::from_secs(1)));
        // Ce que Piper laisse voir quand il annonce son fichier : tout, sauf le dernier tampon de 4 Kio.
        let mut tronque = w[..w.len() - 4096].to_vec();
        assert!(!wav_complet(&tronque));
        assert!(ramener(&mut tronque));
        assert!(wav_complet(&tronque));
        assert!(duree_wav(&tronque).is_some_and(|d| d < Duration::from_secs(1)));
        assert_eq!(entete(&tronque).map(|e| e.taille), Some(44_100 - 4096));
        assert!(!wav_complet(b"RIFF"));
        assert!(!ramener(&mut b"pas un son".to_vec()));
    }

    #[test]
    fn la_bouche_suit_le_niveau_de_la_voix() {
        // 0,2 s de silence, 0,2 s de voix forte (−6 dBFS), 0,2 s de voix douce (−36 dBFS) : cinq pas chacun.
        let mut e = vec![0i16; 4_410];
        e.extend((0..4_410).map(|i| if i % 2 == 0 { 16_000 } else { -16_000 }));
        e.extend((0..4_410).map(|i| if i % 2 == 0 { 520 } else { -520 }));
        let n = enveloppe(&wav(&e));
        assert_eq!(n.len(), 15, "{n:?}");
        assert!(n[..5].iter().all(|&v| v == 0.0), "{n:?}");
        assert!(n[5..10].iter().all(|&v| v == 1.0), "{n:?}");
        assert!(n[10..].iter().all(|&v| v > 0.2 && v < 0.6), "{n:?}");
        assert!(enveloppe(b"pas un son").is_empty());
        // Deux syllabes, deux crêtes (« papa ») : la bouche ouverte sur chacune, fermée juste avant chacune (le p).
        let mut niveaux = vec![0.0f32; 20];
        for (i, v) in [(5, 0.6), (6, 1.0), (7, 0.6), (14, 0.5), (15, 0.9), (16, 0.5)] {
            niveaux[i] = v;
        }
        assert_eq!(cretes(&niveaux), [6, 15]);
        let p = piste_de(niveaux.into(), "papa");
        assert_eq!(&p.formes[3..7], [Forme::Fermee, Forme::Fermee, Forme::Ouverte, Forme::Ouverte]);
        assert_eq!(&p.formes[12..16], [Forme::Fermee, Forme::Fermee, Forme::Ouverte, Forme::Ouverte]);
        // Trois syllabes sur deux crêtes (« bonjour Eve ») : la première et la dernière.
        let mut deux = vec![0.0f32; 12];
        (deux[3], deux[9]) = (1.0, 1.0);
        let p = piste_de(deux.into(), "Bonjour Ève");
        assert_eq!((p.formes[3], p.formes[9]), (Forme::Ronde, Forme::Ouverte));
        // Ce qui s'entend depuis 0,26 s : entre le sixième pas et le septième ; rien ne s'entend, rien.
        faire_entendre(None);
        assert_eq!(bouche_du_moment(), None);
        let piste = piste_de(n, "a");
        if let Ok(mut s) = S_ENTEND.lock() {
            *s = Some((Instant::now() - Duration::from_millis(260), piste));
        }
        assert_eq!(bouche_du_moment().map(|a| (a.niveau > 0.99, a.forme)), Some((true, Forme::Ouverte)));
        // Au tout début du son, la bouche prend son avance peu à peu (les lèvres closes du premier « b » se voient) ;
        // la forme à venir, elle, se lit aussi loin qu'on la demande (la voyelle ronde qui suit).
        let formes: Vec<Forme> = [Forme::Fermee; 6].into_iter().chain([Forme::Ronde; 10]).collect();
        let (niveaux, machoires) = (vec![0.5; 16].into(), vec![0.0; 16].into());
        let piste = Piste { niveaux, machoires, formes: formes.into(), reperes: Arc::from([]) };
        faire_entendre(Some(piste));
        assert_eq!(bouche_a_venir(Duration::from_millis(300)).map(|a| a.forme), Some(Forme::Fermee));
        assert_eq!(forme_a_venir(Duration::from_millis(300)), Some(Forme::Ronde));
        faire_entendre(None);
    }

    #[test]
    fn le_debut_du_son_se_lit_sur_l_horloge_du_lecteur() {
        // `Play()` à t0 : le son part vers t0 + 18 ms.
        let (t0, ms) = (Instant::now(), Duration::from_millis);
        // Une position nulle : le son n'est pas encore parti, rien ne se sait.
        assert_eq!(debut_du_son(t0, Duration::ZERO, Duration::ZERO, None), None);
        // La position du son d'avant, que le lecteur n'a pas encore oubliée : plus longue que le temps passé depuis
        // `Play()`, elle ne compte pas.
        assert_eq!(debut_du_son(t0 + ms(30), ms(30), ms(3_400), None), None);
        // L'heure moins la position…
        let d = debut_du_son(t0 + ms(40), ms(40), ms(20), None);
        assert_eq!(d, Some(t0 + ms(20)));
        // … la plus petite des estimations : une lecture où le fil a tardé (30 ms entre la position et l'heure) ne
        // recule pas le début, une plus juste l'avance, une position nulle ne l'efface pas.
        assert_eq!(debut_du_son(t0 + ms(90), ms(90), ms(40), d), d);
        assert_eq!(debut_du_son(t0 + ms(58), ms(58), ms(40), d), Some(t0 + ms(18)));
        assert_eq!(debut_du_son(t0 + ms(100), ms(100), Duration::ZERO, d), d);
    }

    #[test]
    fn l_image_lit_la_voix_au_retard_de_l_ecran() {
        let (t0, ms) = (Instant::now(), Duration::from_millis);
        // Un son qui partira dans 30 ms, un écran qui montre ses images 50 ms après les avoir calculées : l'image
        // calculée maintenant montre déjà ses 20 premières millisecondes.
        assert_eq!(ecoule_a(t0, t0 + ms(30), ms(50)), Some(ms(20)));
        // Sans retard (le film de l'essai garde chaque image à l'instant de son calcul), rien avant qu'il parte.
        assert_eq!(ecoule_a(t0, t0 + ms(30), Duration::ZERO), None);
        assert_eq!(ecoule_a(t0 + ms(30), t0 + ms(30), Duration::ZERO), Some(Duration::ZERO));
    }

    #[test]
    fn la_machoire_suit_la_voyelle_et_les_levres_se_ferment() {
        use Forme::{Dents as D, Etiree as I, Fermee as F, Ouverte as A};
        // Quatre syllabes, chacune une crête (niveau 1) entre deux creux (0,3) ; une pause de 200 ms après la deuxième.
        let mut niveaux = vec![0.0f32; 2];
        for k in 0..4 {
            niveaux.extend([0.3, 0.7, 1.0, 0.7]);
            if k == 1 {
                niveaux.extend([0.3, 0.0, 0.0, 0.0, 0.0, 0.0]);
            }
        }
        niveaux.extend([0.3, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(cretes(&niveaux), [4, 8, 18, 22]);
        // « la ville. Sam rit » : un a grand ouvert, un i étiré et petit.
        let p = piste_de(niveaux.clone().into(), "la ville. Sam rit");
        assert_eq!((p.formes[4], p.formes[8], p.formes[18], p.formes[22]), (A, I, A, I));
        assert!((p.machoires[4] - 0.85).abs() < 1e-6 && (p.machoires[8] - 0.5).abs() < 1e-6, "{:?}", p.machoires);
        // Entre deux crêtes, la mâchoire se referme avec la voix.
        assert_eq!(p.machoires[6], 0.0, "{:?}", p.machoires);
        // Le m de « Sam » ferme les lèvres, et la mâchoire, au creux qui le suit, jusqu'à ce que « rit » s'entende.
        assert_eq!(&p.formes[18..23], [A, F, F, I, I], "{:?}", p.formes);
        assert!(p.machoires[19..21].iter().all(|&m| m == 0.0));
        // Cinq syllabes avant la pause du texte, pour deux crêtes avant celle de la voix : les syllabes d'après la
        // pause restent à leurs crêtes — elles ne glissent pas d'une phrase à l'autre.
        let p = piste_de(niveaux.clone().into(), "la ville en été. Sam rit");
        assert_eq!((p.formes[18], p.formes[22]), (A, I), "{:?}", p.formes);
        assert_eq!(&p.formes[19..21], [F, F]);
        // « papa » au début de la phrase : les lèvres closes dès le premier pas, jusqu'à ce que le premier a
        // s'entende ; puis de la fin du premier a jusqu'au second.
        let p = piste_de(niveaux.clone().into(), "papa. Sam rit");
        assert_eq!(&p.formes[..8], [F, F, F, A, A, F, F, A], "{:?}", p.formes);
        // « la vie » : la lèvre du bas sous les dents au creux du v.
        let p = piste_de(niveaux.into(), "la vie. Sam rit");
        assert_eq!(&p.formes[4..9], [A, D, D, I, I], "{:?}", p.formes);
        // « la sou mi », sans crête pour « sou » : le m de « mi » ferme les lèvres à son creux, pas à celui du s.
        let n = vec![0.0, 0.0, 0.3, 0.7, 1.0, 0.7, 0.1, 0.6, 0.3, 0.7, 1.0, 0.7, 0.3, 0.0, 0.0];
        assert_eq!(cretes(&n), [4, 10]);
        let p = piste_de(n.into(), "la sou mi");
        assert_eq!(&p.formes[5..10], [A, A, F, F, I], "{:?}", p.formes);
    }

    #[test]
    fn le_silence_final_se_mesure() {
        // Une demi-seconde de voix, puis 0,35 s de silence (avec un peu de souffle), comme les WAV de Piper.
        let mut e = vec![4000i16; 11_025];
        e.extend((0..7_718).map(|i| if i % 2 == 0 { 120 } else { -120 }));
        let s = silence_final(&wav(&e));
        assert!((s.as_secs_f64() - 0.35).abs() < 0.001, "{s:?}");
        assert_eq!(silence_final(&wav(&[4000; 100])), Duration::ZERO);
        assert_eq!(silence_final(&wav(&[0; 22_050])), Duration::from_secs(1));
        assert_eq!(silence_final(b"pas un son"), Duration::ZERO);
    }

    #[test]
    fn la_douceur_adoucit_et_baisse_le_son() {
        let mut w = wav(&vec![10_000; 2_000]);
        adoucir(&mut w, Douceur::de(1));
        let e = entete(&w).unwrap();
        let dernier = i16::from_le_bytes([w[e.debut + 2 * 1999], w[e.debut + 2 * 1999 + 1]]);
        let premier = i16::from_le_bytes([w[e.debut], w[e.debut + 1]]);
        // Le passe-bas monte en douceur (pas de claquement), puis le gain de 0,85.
        assert!(premier < 8_000, "{premier}");
        assert!((8_400..=8_600).contains(&dernier), "{dernier}");
        let mut telle = wav(&[10_000; 10]);
        let avant = telle.clone();
        adoucir(&mut telle, Douceur::de(0));
        assert_eq!(telle, avant);
    }

    #[test]
    fn la_voix_choisie_dit_son_moteur_et_son_repli() {
        assert_eq!(moteur("piper:fr_FR-upmc-medium#0"), Moteur::Piper("fr_FR-upmc-medium".into(), Some(0)));
        assert_eq!(moteur("piper:fr_FR-siwis-medium"), Moteur::Piper("fr_FR-siwis-medium".into(), None));
        assert_eq!(moteur("windows:Microsoft Caroline"), Moteur::Windows("Microsoft Caroline".into()));
        assert_eq!(moteur(""), Moteur::Aucun);
        let v = |id: &str, femme| VoixDispo { id: id.into(), nom: id.into(), femme };
        let liste = vec![
            v("piper:fr_FR-upmc-medium#0", true),
            v("windows:Microsoft Claude", false),
            v("windows:Microsoft Caroline", true),
        ];
        assert_eq!(voix_de_repli(&liste).map(|v| v.id.as_str()), Some("windows:Microsoft Caroline"));
        assert_eq!(voix_de_repli(&liste[..2]).map(|v| v.id.as_str()), Some("windows:Microsoft Claude"));
        assert!(voix_de_repli(&liste[..1]).is_none());
        assert_eq!(choisir_dans(&liste, "windows:Microsoft Claude"), "windows:Microsoft Claude");
        assert_eq!(choisir_dans(&liste, "piper:disparue"), VOIX_PAR_DEFAUT);
    }

    /// Attend une trace qui convient (au plus `delai`), en gardant toutes celles qui passent.
    fn attendre(rt: &Receiver<Trace>, vues: &mut Vec<Trace>, delai: Duration, voulue: impl Fn(&Trace) -> bool) -> bool {
        let fin = Instant::now() + delai;
        while let Some(reste) = fin.checked_duration_since(Instant::now()) {
            match rt.recv_timeout(reste) {
                Ok(t) => {
                    let ok = voulue(&t);
                    vues.push(t);
                    if ok {
                        return true;
                    }
                }
                Err(_) => return false,
            }
        }
        false
    }

    /// Sur le poste, sans un mot (seul le dixième de seconde de silence des lecteurs se joue) : Piper se lance avant
    /// que les lecteurs de Windows s'ouvrent, et se charge pendant ce temps.
    /// `cargo test --release -- --ignored --nocapture la_voix_se_prechauffe`.
    #[test]
    #[ignore]
    fn la_voix_se_prechauffe() {
        let (tx, rx) = mpsc::channel();
        let (tt, rt) = mpsc::channel();
        let t0 = Instant::now();
        let v = lancer(&Reglages::default(), tx, Some(tt));
        let mut vues = Vec::new();
        assert!(
            attendre(&rt, &mut vues, Duration::from_secs(20), |t| matches!(t, Trace::PiperPret(_))),
            "Piper pas prêt"
        );
        let lance = vues.iter().find_map(|t| if let Trace::PiperLance(a) = t { Some(*a) } else { None });
        let pret = vues.iter().find_map(|t| if let Trace::PiperPret(a) = t { Some(*a) } else { None });
        let (lance, pret) = (lance.expect("Piper pas lancé"), pret.expect("Piper pas prêt"));
        println!(
            "Piper lancé après {:.0} ms ; prêt après {:.0} ms",
            (lance - t0).as_secs_f64() * 1000.0,
            (pret - t0).as_secs_f64() * 1000.0
        );
        let voix = rx.try_iter().find_map(|e| if let Evt::Voix { choisie, .. } = e { Some(choisie) } else { None });
        println!("voix : {voix:?}");
        assert_eq!(voix.as_deref(), Some(VOIX_PAR_DEFAUT));
        drop(v);
    }

    /// Sur le poste, la voix se fait entendre une quinzaine de secondes :
    /// `cargo test --release -- --ignored --nocapture la_voix_tuile_les_phrases`.
    /// Trois phrases données à 300 ms d'écart (la troisième longue : Piper la prépare encore quand elle commence), une
    /// coupure pendant la troisième, puis une quatrième ; le délai avant la première phrase, les trous entre les
    /// phrases, la coupure et la phrase d'après se mesurent.
    #[test]
    #[ignore]
    fn la_voix_tuile_les_phrases() {
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let (tx, rx) = mpsc::channel();
        let (tt, rt) = mpsc::channel();
        let t0 = Instant::now();
        let v = lancer(&Reglages::default(), tx, Some(tt));
        let mut vues = Vec::new();
        assert!(
            attendre(&rt, &mut vues, Duration::from_secs(20), |t| matches!(t, Trace::PiperPret(_))),
            "Piper pas prêt"
        );
        println!("Piper prêt en {:.0} ms", ms(t0.elapsed()));

        let phrases = [
            "Bonjour Martin, ceci est un essai de la voix d'Eve.",
            "La deuxième phrase arrive trois cents millisecondes plus tard.",
            "La troisième est longue, et elle sera coupée. Piper la prépare pendant que la première se dit. \
             Puis il continue avec la suite de la réponse. Cette phrase ne sera sans doute jamais entendue. \
             Celle-ci non plus, bien entendu. La file en contient encore plusieurs autres. \
             Le chantier de la rue Principale avance comme prévu. Le gypse arrive mardi matin au chantier. \
             La soumission de Toitures Exemple attend sa signature. Il faudra rappeler le fournisseur vendredi. \
             Le calendrier de la semaine prochaine est déjà bien rempli. Martin passera au chalet jeudi. \
             Julie finit la finition intérieure de la cuisine. Et la dernière ferme la marche, sans un mot.",
        ];
        let mut donnees = Vec::new();
        for (i, p) in phrases.iter().enumerate() {
            if i > 0 {
                thread::sleep(Duration::from_millis(300));
            }
            donnees.push(Instant::now());
            v.dire(i as u64 + 1, p);
        }
        assert!(attendre(&rt, &mut vues, Duration::from_secs(40), |t| matches!(t, Trace::Debut { id: 3, .. })));
        thread::sleep(Duration::from_millis(500));
        let t_taire = Instant::now();
        v.taire();
        let t_quatre = Instant::now();
        v.dire(4, "La coupure est faite, et voici la phrase d'après.");
        assert!(attendre(&rt, &mut vues, Duration::from_secs(40), |t| matches!(t, Trace::Fin { id: 4, .. })));
        thread::sleep(Duration::from_millis(100));
        drop(v);
        vues.extend(rt.try_iter());

        // Le premier son donné au lecteur, et le premier entendu, d'une parole.
        let donne = |id: u64| {
            vues.iter().find_map(|t| if let Trace::Debut { id: i, a } = t { (*i == id).then_some(*a) } else { None })
        };
        let entendu = |id: u64| {
            vues.iter().find_map(|t| if let Trace::Son { id: i, a } = t { (*i == id).then_some(*a) } else { None })
        };
        let premier = entendu(1).expect("la phrase 1 ne s'est pas fait entendre");
        println!(
            "délai avant la première phrase : {:.0} ms (au lecteur après {:.0} ms)",
            ms(premier - donnees[0]),
            donne(1).map_or(f64::NAN, |d| ms(d - donnees[0]))
        );
        // Les trous, de la fin d'un WAV (silence final compris : 0,35 s, la pause voulue entre les phrases) au son
        // suivant, donné au lecteur puis entendu ; négatif, le suivant s'entend avant la fin de ce silence.
        let ecart = |a: Instant, b: Instant| if a >= b { ms(a - b) } else { -ms(b - a) };
        let mut fin_precedente: Option<(u64, Instant)> = None;
        let mut trous = Vec::new();
        for t in &vues {
            match t {
                Trace::Debut { id, a } => {
                    if let Some((p, f)) = fin_precedente {
                        print!("  trou {p} → {id} : au lecteur à {:+.0} ms", ecart(*a, f));
                    }
                }
                Trace::Son { id, a } => {
                    if let Some((_, f)) = fin_precedente.take() {
                        println!(", entendu à {:+.0} ms", ecart(*a, f));
                        trous.push((*id, ecart(*a, f)));
                    }
                }
                Trace::Fin { id, a } => fin_precedente = Some((*id, *a)),
                Trace::Coupee(_) => fin_precedente = None,
                _ => {}
            }
        }
        let coupee = vues.iter().find_map(|t| if let Trace::Coupee(a) = t { Some(*a) } else { None });
        let coupure = coupee.map(|c| c - t_taire).expect("la coupure n'a rien arrêté");
        println!("coupure : {:.1} ms après taire()", ms(coupure));
        let relance = vues.iter().find_map(|t| if let Trace::PiperRelance(a) = t { Some(*a) } else { None });
        if let Some(r) = relance {
            let pret = vues.iter().rev().find_map(|t| if let Trace::PiperPret(a) = t { Some(*a) } else { None });
            println!("Piper relancé après la coupure ; prêt {:.0} ms plus tard", pret.map_or(f64::NAN, |p| ms(p - r)));
        } else {
            println!("Piper n'avait plus rien de coupé en préparation : pas relancé");
        }
        let quatre = entendu(4).expect("la phrase 4, donnée après la coupure, ne s'est pas fait entendre");
        println!("phrase d'après la coupure : entendue {:.0} ms après dire()", ms(quatre - t_quatre));

        let finies: Vec<u64> =
            rx.try_iter().filter_map(|e| if let Evt::VoixFinie(id) = e { Some(id) } else { None }).collect();
        println!("VoixFinie : {finies:?}");
        assert_eq!(finies, vec![1, 2, 3, 4]);
        assert!(coupure < Duration::from_millis(50), "la coupure a pris {:.1} ms", ms(coupure));
        for (id, trou) in trous {
            assert!(trou.abs() < 150.0, "un trou de {trou:.0} ms avant {id}");
        }
    }
}
