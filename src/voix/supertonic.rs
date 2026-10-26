//! Supertonic 2 (Supertone : le code sous licence MIT, le modèle sous OpenRAIL-M), une voix neuronale de plus, sur le
//! processeur. Mesuré le 2026-10-26 sur un i5-3470 (sans AVX2), contre Jessica (Piper) : un juge automatique (UTMOS22)
//! le note de 3,6 à 3,9 contre 3,3, l'oreille d'Eve en entend 2,8 à 6,3 % des mots de travers contre 7,7, et il
//! calcule une phrase en 0,21 à 0,28 fois sa durée (Piper : 0,14).
//!
//! Quatre graphes ONNX — la durée de la phrase, l'encodeur du texte, l'estimateur d'un débruitage en huit pas, le
//! vocodeur —, exécutés par ONNX Runtime : sa bibliothèque (`onnxruntime.dll`, 1.31) est chargée à l'exécution, depuis
//! le dossier de la voix ; celle de Piper (1.14, de 2023) ne lit pas ces graphes. Le moteur parle la langue de Piper,
//! pour que le fil de la voix le mène de même ([`super::Piper`]) : une ligne JSON par phrase (`text`, `output_file`),
//! puis, la phrase dite, le nom du WAV écrit, dans l'ordre. Il travaille dans un fil à lui, une phrase à la fois ; ce
//! que le fil de la voix a coupé, il le saute ([`Supertonic::oubli`]) sans tout recharger.
//!
//! Ses fichiers, dans le dossier `supertonic` des ressources : `onnx\` (les quatre graphes, `tts.json`,
//! `unicode_indexer.json`), `voice_styles\` (F1 à F5, les voix de femme), `onnxruntime\lib\onnxruntime.dll`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use serde_json::Value;

/// Le débit d'office de Supertonic (1,05), celui où ses huit phrases d'essai durent ce que dure Jessica à son débit
/// d'office, 0,85 : le débit d'Eve s'y rapporte ([`vitesse`]).
const VITESSE_DE_JESSICA: f32 = 1.05;
/// Les pas du débruitage : 8, ceux de l'exemple de Supertone ; moins, la voix se dégrade.
const PAS: usize = 8;
/// Au-delà, un texte se dit en morceaux, une pause entre eux (l'exemple de Supertone en fait autant).
const LONGUEUR_D_UN_MORCEAU: usize = 300;
/// La pause entre deux morceaux d'un long texte.
const PAUSE_ENTRE_MORCEAUX: f32 = 0.3;
/// La phrase du tour à vide, au chargement : d'une longueur ordinaire, pour qu'ONNX Runtime y prépare sa mémoire.
const PREMIERE_PHRASE: &str =
    "Bonjour, je suis prête : je t'écoute, et je te réponds dès que tu me parles, pour un rendez-vous ou une facture.";

/// Le dossier de la voix : `supertonic` des ressources (une jonction vers le dossier commun, comme `voix`).
pub fn dossier() -> PathBuf {
    crate::reglages::ressource("supertonic")
}

/// ONNX Runtime, dans le dossier de la voix : `onnxruntime\lib\onnxruntime.dll`, comme sa publication officielle le
/// range (le kit n'en garde que la bibliothèque, sa licence et ses avis de tiers).
fn bibliotheque(dossier: &Path) -> PathBuf {
    dossier.join("onnxruntime").join("lib").join("onnxruntime.dll")
}

/// Les voix de Supertonic posées sur le poste (« F1 »… « M5 »), si le modèle et ONNX Runtime y sont.
pub fn styles() -> Vec<String> {
    let d = dossier();
    let complet = ["duration_predictor", "text_encoder", "vector_estimator", "vocoder"]
        .iter()
        .all(|n| d.join("onnx").join(format!("{n}.onnx")).is_file())
        && ["tts.json", "unicode_indexer.json"].iter().all(|n| d.join("onnx").join(n).is_file())
        && bibliotheque(&d).is_file();
    if !complet {
        return Vec::new();
    }
    let mut s: Vec<String> = fs::read_dir(d.join("voice_styles"))
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .filter_map(|p| p.file_stem().map(|n| n.to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    s.sort();
    s
}

/// La vitesse de Supertonic pour le débit d'Eve : celle de l'exemple au débit de Jessica (0,85), en proportion.
pub fn vitesse(debit: f64) -> f32 {
    VITESSE_DE_JESSICA * (debit.clamp(0.5, 2.0) / 0.85) as f32
}

/// Le moteur en marche, vu du fil de la voix : ce qu'il lui faut pour le mener comme Piper.
pub struct Supertonic {
    /// Les lignes JSON des phrases à dire.
    pub entree: Sender<String>,
    /// Le nom de chaque WAV écrit, dans l'ordre des phrases ; déconnecté : le moteur s'est arrêté.
    pub sorties: Receiver<String>,
    /// Le modèle est chargé.
    pub pret: Arc<AtomicBool>,
    /// La dernière erreur du moteur.
    pub erreur: Arc<Mutex<String>>,
    /// Les phrases jusqu'à ce numéro sont coupées : le moteur les saute (celle qu'il dit, il la finit).
    pub oubli: Arc<AtomicU64>,
}

impl Supertonic {
    /// Lance le moteur dans son fil : il charge le modèle (quelques secondes), puis dit les phrases qu'il reçoit, une à
    /// une, dans `dossier`.
    pub fn lancer(style: &str, vitesse: f32, dossier_des_wav: &Path) -> Supertonic {
        let (entree, lignes) = mpsc::channel::<String>();
        let (tx, sorties) = mpsc::channel::<String>();
        let pret = Arc::new(AtomicBool::new(false));
        let erreur = Arc::new(Mutex::new(String::new()));
        let oubli = Arc::new(AtomicU64::new(0));
        let (p, e, o) = (pret.clone(), erreur.clone(), oubli.clone());
        let (style, dossier_des_wav) = (style.to_string(), dossier_des_wav.to_path_buf());
        thread::spawn(move || {
            let dire_l_erreur = |m: String| {
                crate::journal::ecrire(&format!("supertonic : {m}"));
                if let Ok(mut x) = e.lock() {
                    *x = m;
                }
            };
            let mut modele = match Modele::charger(&dossier(), &style, vitesse) {
                Ok(m) => m,
                Err(m) => {
                    dire_l_erreur(m);
                    return;
                }
            };
            // Un tour à vide : ONNX Runtime prépare ses calculs à la première phrase (mesuré : les premières se
            // calculent deux fois plus lentement), pendant que rien ne se dit encore.
            let _ = modele.dire(PREMIERE_PHRASE);
            p.store(true, Ordering::SeqCst);
            for ligne in lignes {
                let Ok(phrase) = serde_json::from_str::<Value>(&ligne) else { continue };
                let texte = phrase.get("text").and_then(Value::as_str).unwrap_or_default();
                let nom = phrase.get("output_file").and_then(Value::as_str).unwrap_or_default().to_string();
                let numero = phrase.get("numero").and_then(Value::as_u64).unwrap_or(u64::MAX);
                // Coupée entre-temps : rien à dire, mais le nom revient à son rang (le fil de la voix ne lit pas le WAV
                // d'une phrase coupée).
                if numero > o.load(Ordering::SeqCst) {
                    match modele.dire(texte) {
                        Ok(son) => {
                            let wav = super::wav_mono(&en_entiers(&son), modele.taux);
                            if let Err(err) = fs::write(dossier_des_wav.join(&nom), wav) {
                                dire_l_erreur(format!("le WAV ne s'écrit pas ({err})"));
                            }
                        }
                        Err(m) => dire_l_erreur(m),
                    }
                }
                if tx.send(nom).is_err() {
                    break;
                }
            }
        });
        Supertonic { entree, sorties, pret, erreur, oubli }
    }
}

/// Supertonic parle 9,5 dB plus bas que Piper (mesuré le 2026-10-26 sur huit phrases : −24,7 dBFS sur la parole,
/// contre −15,1 ; ses crêtes à −13 dBFS) : ramené au même niveau, ses crêtes vers −3,5 dBFS. Un gain fixe garde ses
/// nuances d'une phrase à l'autre.
const GAIN_DE_NIVEAU: f32 = 3.0;

/// Des échantillons de -1 à 1 en PCM 16 bits, au niveau de Piper.
fn en_entiers(son: &[f32]) -> Vec<i16> {
    son.iter().map(|&v| ((v * GAIN_DE_NIVEAU).clamp(-1.0, 1.0) * 32_767.0) as i16).collect()
}

/// L'environnement d'ONNX Runtime, un par processus : la bibliothèque chargée une fois.
static ENVIRONNEMENT: OnceLock<Result<Environment, String>> = OnceLock::new();

fn environnement(dll: &Path) -> Result<&'static Environment, String> {
    ENVIRONNEMENT
        .get_or_init(|| {
            ort::init_from(dll)
                .map_err(|e| format!("ONNX Runtime ne se charge pas ({e})"))?
                .with_name("eve")
                .build()
                .map_err(|e| format!("ONNX Runtime ne démarre pas ({e})"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Un tenseur : sa forme, ses données.
struct Bloc {
    forme: Vec<i64>,
    donnees: Vec<f32>,
}

/// Le modèle chargé, et sa voix.
struct Modele {
    duree: Session,
    encodeur: Session,
    estimateur: Session,
    vocodeur: Session,
    /// L'indice de chaque caractère (son point de code), -1 s'il est inconnu.
    indexeur: Vec<i64>,
    taux: u32,
    /// Les échantillons d'un pas de la représentation latente, et sa largeur.
    morceau: usize,
    largeur: usize,
    style_ttl: Bloc,
    style_dp: Bloc,
    vitesse: f32,
    hasard: u64,
}

fn json(chemin: &Path) -> Result<Value, String> {
    let t = fs::read_to_string(chemin).map_err(|e| format!("{} ne se lit pas ({e})", chemin.display()))?;
    serde_json::from_str(&t).map_err(|e| format!("{} n'est pas du JSON ({e})", chemin.display()))
}

/// Les nombres d'un tableau JSON imbriqué, à plat.
fn a_plat(v: &Value, sortie: &mut Vec<f32>) {
    match v {
        Value::Array(a) => a.iter().for_each(|x| a_plat(x, sortie)),
        Value::Number(n) => sortie.push(n.as_f64().unwrap_or(0.0) as f32),
        _ => {}
    }
}

fn bloc(v: &Value) -> Result<Bloc, String> {
    let forme: Vec<i64> = v
        .get("dims")
        .and_then(Value::as_array)
        .map(|d| d.iter().filter_map(Value::as_i64).collect())
        .ok_or("un style sans dimensions")?;
    let mut donnees = Vec::new();
    a_plat(v.get("data").ok_or("un style sans données")?, &mut donnees);
    if forme.iter().product::<i64>() != donnees.len() as i64 {
        return Err("un style dont les données ne font pas sa forme".into());
    }
    Ok(Bloc { forme, donnees })
}

fn tenseur<T: ort::value::PrimitiveTensorElementType + Clone + std::fmt::Debug + 'static>(
    forme: Vec<i64>,
    donnees: Vec<T>,
) -> Result<Tensor<T>, String> {
    Tensor::from_array((forme, donnees)).map_err(|e| e.to_string())
}

impl Modele {
    fn charger(dossier: &Path, style: &str, vitesse: f32) -> Result<Modele, String> {
        let env = environnement(&bibliotheque(dossier))?;
        let onnx = dossier.join("onnx");
        let fils = thread::available_parallelism().map_or(4, |n| n.get().min(4));
        let session = |nom: &str| -> Result<Session, String> {
            Session::builder(env)
                .map_err(|e| e.to_string())?
                .with_intra_threads(fils)
                .map_err(|e| e.to_string())?
                .commit_from_file(onnx.join(format!("{nom}.onnx")))
                .map_err(|e| format!("{nom}.onnx ne se charge pas ({e})"))
        };
        let cfg = json(&onnx.join("tts.json"))?;
        let nombre = |a: &str, b: &str| cfg.get(a).and_then(|x| x.get(b)).and_then(Value::as_u64);
        let (Some(taux), Some(base), Some(facteur), Some(dim)) = (
            nombre("ae", "sample_rate"),
            nombre("ae", "base_chunk_size"),
            nombre("ttl", "chunk_compress_factor"),
            nombre("ttl", "latent_dim"),
        ) else {
            return Err("tts.json ne dit pas sa cadence ni ses dimensions".into());
        };
        let indexeur: Vec<i64> = json(&onnx.join("unicode_indexer.json"))?
            .as_array()
            .map(|a| a.iter().map(|x| x.as_i64().unwrap_or(-1)).collect())
            .ok_or("unicode_indexer.json n'est pas un tableau")?;
        let voix = json(&dossier.join("voice_styles").join(format!("{style}.json")))?;
        let style_ttl = bloc(voix.get("style_ttl").ok_or("un style sans style_ttl")?)?;
        let style_dp = bloc(voix.get("style_dp").ok_or("un style sans style_dp")?)?;
        let graine = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos());
        Ok(Modele {
            duree: session("duration_predictor")?,
            encodeur: session("text_encoder")?,
            estimateur: session("vector_estimator")?,
            vocodeur: session("vocoder")?,
            indexeur,
            taux: taux as u32,
            morceau: (base * facteur) as usize,
            largeur: (dim * facteur) as usize,
            style_ttl,
            style_dp,
            vitesse,
            hasard: (graine as u64) | 1,
        })
    }

    /// Un nombre tiré d'une loi normale (Box-Muller, sur un xorshift).
    fn normale(&mut self) -> f32 {
        let mut uniforme = || {
            self.hasard ^= self.hasard << 13;
            self.hasard ^= self.hasard >> 7;
            self.hasard ^= self.hasard << 17;
            ((self.hasard >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        let (u, v) = (uniforme(), uniforme());
        ((-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()) as f32
    }

    /// Un texte dit : ses morceaux l'un après l'autre, une courte pause entre eux.
    fn dire(&mut self, texte: &str) -> Result<Vec<f32>, String> {
        let mut son = Vec::new();
        for (k, m) in morceaux(texte, LONGUEUR_D_UN_MORCEAU).iter().enumerate() {
            if k > 0 {
                son.extend(std::iter::repeat_n(0.0, (PAUSE_ENTRE_MORCEAUX * self.taux as f32) as usize));
            }
            son.extend(sans_silence_de_tete(self.dire_un_morceau(m)?, self.taux));
        }
        Ok(son)
    }

    fn dire_un_morceau(&mut self, texte: &str) -> Result<Vec<f32>, String> {
        let erreur = |e: ort::Error| e.to_string();
        let ids: Vec<i64> = preparer(texte)
            .chars()
            .filter_map(|c| self.indexeur.get(c as usize).copied())
            .filter(|&i| i >= 0)
            .collect();
        let n = ids.len() as i64;
        let ids = tenseur(vec![1, n], ids)?;
        let masque = tenseur(vec![1, 1, n], vec![1.0f32; n as usize])?;
        let style_dp = tenseur(self.style_dp.forme.clone(), self.style_dp.donnees.clone())?;
        let style_ttl = tenseur(self.style_ttl.forme.clone(), self.style_ttl.donnees.clone())?;
        let duree = {
            let s = self
                .duree
                .run(ort::inputs!["text_ids" => &ids, "style_dp" => &style_dp, "text_mask" => &masque])
                .map_err(erreur)?;
            let (_, d) = s[0].try_extract_tensor::<f32>().map_err(erreur)?;
            d.first().copied().unwrap_or(0.0) / self.vitesse
        };
        let (forme_emb, emb) = {
            let s = self
                .encodeur
                .run(ort::inputs!["text_ids" => &ids, "style_ttl" => &style_ttl, "text_mask" => &masque])
                .map_err(erreur)?;
            let (f, d) = s[0].try_extract_tensor::<f32>().map_err(erreur)?;
            (f.to_vec(), d.to_vec())
        };
        let emb = tenseur(forme_emb, emb)?;
        let echantillons = (duree.max(0.1) * self.taux as f32) as usize;
        let pas_latents = echantillons.div_ceil(self.morceau).max(1);
        let forme = vec![1, self.largeur as i64, pas_latents as i64];
        let masque_latent = tenseur(vec![1, 1, pas_latents as i64], vec![1.0f32; pas_latents])?;
        let total = tenseur(vec![1], vec![PAS as f32])?;
        let mut x: Vec<f32> = (0..self.largeur * pas_latents).map(|_| self.normale()).collect();
        for k in 0..PAS {
            let bruit = tenseur(forme.clone(), x)?;
            let pas = tenseur(vec![1], vec![k as f32])?;
            let s = self
                .estimateur
                .run(ort::inputs![
                    "noisy_latent" => &bruit,
                    "text_emb" => &emb,
                    "style_ttl" => &style_ttl,
                    "text_mask" => &masque,
                    "latent_mask" => &masque_latent,
                    "current_step" => &pas,
                    "total_step" => &total
                ])
                .map_err(erreur)?;
            x = s[0].try_extract_tensor::<f32>().map_err(erreur)?.1.to_vec();
        }
        let latent = tenseur(forme, x)?;
        let s = self.vocodeur.run(ort::inputs!["latent" => &latent]).map_err(erreur)?;
        let (_, son) = s[0].try_extract_tensor::<f32>().map_err(erreur)?;
        Ok(son[..echantillons.min(son.len())].to_vec())
    }
}

/// Le son sans son silence de tête, 20 ms gardées avant le premier son (à 40 dB sous la crête : une consonne douce,
/// un « s », reste entière). Supertonic commence chaque phrase par 0,16 à 0,21 s de silence, et parfois une seconde
/// (mesuré le 2026-10-26, contre 0 à 0,06 s pour Piper) : le premier mot en serait d'autant retardé, et chaque pause
/// entre deux phrases allongée.
fn sans_silence_de_tete(mut son: Vec<f32>, taux: u32) -> Vec<f32> {
    let crete = son.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    let seuil = crete * 0.01;
    if let Some(premier) = son.iter().position(|v| v.abs() > seuil) {
        son.drain(..premier.saturating_sub(taux as usize / 50));
    }
    son
}

/// Un long texte en morceaux d'au plus `max` caractères, coupés après une fin de phrase (sinon une virgule, sinon un
/// blanc) : Supertonic ne dit bien que des textes courts.
fn morceaux(texte: &str, max: usize) -> Vec<String> {
    let mut sortie = Vec::new();
    let mut reste = texte.trim();
    while reste.chars().count() > max {
        let debut: String = reste.chars().take(max).collect();
        let coupe = [". ", "! ", "? ", "; ", ", ", " "]
            .iter()
            .find_map(|m| debut.rfind(m).filter(|&p| p > 0).map(|p| p + m.len()))
            .unwrap_or(debut.len());
        sortie.push(reste[..coupe].trim().to_string());
        reste = reste[coupe..].trim_start();
    }
    if !reste.is_empty() {
        sortie.push(reste.to_string());
    }
    sortie
}

/// Les lettres accentuées, décomposées en leur lettre et leur accent (la décomposition NFKD que Supertonic attend :
/// son indexeur ne connaît que les accents seuls).
const DECOMPOSITIONS: [(char, char, char); 52] = [
    ('à', 'a', '\u{300}'),
    ('á', 'a', '\u{301}'),
    ('â', 'a', '\u{302}'),
    ('ã', 'a', '\u{303}'),
    ('ä', 'a', '\u{308}'),
    ('å', 'a', '\u{30a}'),
    ('ç', 'c', '\u{327}'),
    ('è', 'e', '\u{300}'),
    ('é', 'e', '\u{301}'),
    ('ê', 'e', '\u{302}'),
    ('ë', 'e', '\u{308}'),
    ('ì', 'i', '\u{300}'),
    ('í', 'i', '\u{301}'),
    ('î', 'i', '\u{302}'),
    ('ï', 'i', '\u{308}'),
    ('ñ', 'n', '\u{303}'),
    ('ò', 'o', '\u{300}'),
    ('ó', 'o', '\u{301}'),
    ('ô', 'o', '\u{302}'),
    ('õ', 'o', '\u{303}'),
    ('ö', 'o', '\u{308}'),
    ('ù', 'u', '\u{300}'),
    ('ú', 'u', '\u{301}'),
    ('û', 'u', '\u{302}'),
    ('ü', 'u', '\u{308}'),
    ('ÿ', 'y', '\u{308}'),
    ('À', 'A', '\u{300}'),
    ('Á', 'A', '\u{301}'),
    ('Â', 'A', '\u{302}'),
    ('Ã', 'A', '\u{303}'),
    ('Ä', 'A', '\u{308}'),
    ('Å', 'A', '\u{30a}'),
    ('Ç', 'C', '\u{327}'),
    ('È', 'E', '\u{300}'),
    ('É', 'E', '\u{301}'),
    ('Ê', 'E', '\u{302}'),
    ('Ë', 'E', '\u{308}'),
    ('Ì', 'I', '\u{300}'),
    ('Í', 'I', '\u{301}'),
    ('Î', 'I', '\u{302}'),
    ('Ï', 'I', '\u{308}'),
    ('Ñ', 'N', '\u{303}'),
    ('Ò', 'O', '\u{300}'),
    ('Ó', 'O', '\u{301}'),
    ('Ô', 'O', '\u{302}'),
    ('Õ', 'O', '\u{303}'),
    ('Ö', 'O', '\u{308}'),
    ('Ù', 'U', '\u{300}'),
    ('Ú', 'U', '\u{301}'),
    ('Û', 'U', '\u{302}'),
    ('Ü', 'U', '\u{308}'),
    ('Ÿ', 'Y', '\u{308}'),
];

/// Le texte comme Supertone le prépare pour son modèle (l'exemple `helper.py`) : décomposé, ses tirets, guillemets et
/// apostrophes simplifiés, sans espace avant la ponctuation, une ponctuation au bout, dans la balise de la langue.
pub fn preparer(texte: &str) -> String {
    let mut s = String::with_capacity(texte.len() + 16);
    for c in texte.chars() {
        match c {
            '\u{a0}' | '\u{202f}' | '\u{2009}' => s.push(' '),
            '–' | '‑' | '—' => s.push('-'),
            '_' | '[' | ']' | '|' | '/' | '#' | '→' | '←' => s.push(' '),
            '“' | '”' => s.push('"'),
            '‘' | '’' | '´' | '`' => s.push('\''),
            '♥' | '☆' | '♡' | '©' | '\\' => {}
            '…' => s.push_str("..."),
            'Œ' => s.push_str("OE"),
            'æ' => s.push_str("ae"),
            'Æ' => s.push_str("AE"),
            c => match DECOMPOSITIONS.iter().find(|d| d.0 == c) {
                Some(&(_, lettre, accent)) => {
                    s.push(lettre);
                    s.push(accent);
                }
                None => s.push(c),
            },
        }
    }
    let mut s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    for p in [",", ".", "!", "?", ";", ":", "'"] {
        s = s.replace(&format!(" {p}"), p);
    }
    while s.contains("\"\"") {
        s = s.replace("\"\"", "\"");
    }
    while s.contains("''") {
        s = s.replace("''", "'");
    }
    if !s.ends_with(|c: char| ".!?;:,'\")]}…»".contains(c)) {
        s.push('.');
    }
    format!("<fr>{s}</fr>")
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn le_texte_se_prepare_comme_supertone_le_fait() {
        assert_eq!(preparer("Bonjour Sam !"), "<fr>Bonjour Sam!</fr>");
        // Les accents à part, comme la décomposition NFKD les met : « é » est un e et un accent aigu.
        assert_eq!(preparer("Déjà à l’école"), "<fr>De\u{301}ja\u{300} a\u{300} l'e\u{301}cole.</fr>");
        assert_eq!(preparer("Le plan — « final » :  oui"), "<fr>Le plan - « final »: oui.</fr>");
        assert_eq!(preparer("Attends…"), "<fr>Attends...</fr>");
    }

    #[test]
    fn un_long_texte_se_dit_en_morceaux() {
        let phrase = "Une phrase qui se dit en entier, avec une virgule au milieu. ";
        let long = phrase.repeat(8);
        let m = morceaux(&long, 300);
        assert!(m.len() >= 2, "{m:?}");
        assert!(m.iter().all(|x| x.chars().count() <= 300 && x.ends_with('.')), "{m:?}");
        assert_eq!(m.join(" "), long.trim());
        assert_eq!(morceaux("Court.", 300), vec!["Court."]);
    }

    #[test]
    fn le_silence_de_tete_se_rogne() {
        // 0,5 s de silence (un souffle à -60 dB), puis la voix : il reste 20 ms avant elle.
        let mut son = vec![0.001f32; 22_050];
        son.extend(std::iter::repeat_n(0.5f32, 4_410));
        let rogne = sans_silence_de_tete(son, 44_100);
        assert_eq!(rogne.len(), 882 + 4_410);
        assert_eq!(rogne[882], 0.5);
        assert!(sans_silence_de_tete(Vec::new(), 44_100).is_empty());
        assert_eq!(sans_silence_de_tete(vec![0.5; 10], 44_100).len(), 10);
    }

    #[test]
    fn le_debit_d_eve_se_rapporte_a_celui_de_jessica() {
        assert!((vitesse(0.85) - 1.05).abs() < 1e-6);
        assert!(vitesse(1.0) > vitesse(0.85));
    }

    /// Sur le poste : les phrases d'un corpus (une par ligne) dites par Supertonic, un WAV chacune, et leur temps de
    /// calcul. `EVE_SUPERTONIC=<dossier du modèle> EVE_CORPUS=<phrases.txt> EVE_SORTIE=<dossier> [EVE_STYLE=F3]
    /// cargo test --release --bin eve -- --ignored --nocapture supertonic_dit_un_corpus`.
    #[test]
    #[ignore]
    fn supertonic_dit_un_corpus() {
        let var = |n: &str| std::env::var(n).ok();
        let (Some(d), Some(c), Some(s)) = (var("EVE_SUPERTONIC"), var("EVE_CORPUS"), var("EVE_SORTIE")) else { return };
        let style = var("EVE_STYLE").unwrap_or_else(|| "F3".into());
        let t0 = std::time::Instant::now();
        let mut m = Modele::charger(Path::new(&d), &style, VITESSE_DE_JESSICA).expect("le modèle ne se charge pas");
        let charge = t0.elapsed().as_secs_f32();
        m.dire(PREMIERE_PHRASE).expect("le tour à vide");
        println!("modele charge en {charge:.1} s, tour a vide en {:.1} s", t0.elapsed().as_secs_f32() - charge);
        let corpus = fs::read_to_string(c).expect("le corpus ne se lit pas");
        for (k, texte) in corpus.lines().filter(|l| !l.trim().is_empty()).enumerate() {
            let t0 = std::time::Instant::now();
            let son = m.dire(texte).expect("la phrase ne se dit pas");
            let (calcul, duree) = (t0.elapsed().as_secs_f32(), son.len() as f32 / m.taux as f32);
            let wav = crate::voix::wav_mono(&en_entiers(&son), m.taux);
            fs::write(Path::new(&s).join(format!("phrase-{}.wav", k + 1)), wav).expect("le WAV ne s'écrit pas");
            println!("phrase {} : {duree:.2} s de son en {calcul:.2} s (facteur {:.3})", k + 1, calcul / duree);
        }
    }
}
