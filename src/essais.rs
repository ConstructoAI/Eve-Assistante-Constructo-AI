//! Les essais du poste, sans fenêtre ni micro :
//! - `eve --voix` : les voix du poste ;
//! - `eve --essai-voix [--id ID] texte…` : la voix dit un texte ;
//! - `eve --essai-oreille fichier.wav [modèle]` : l'oreille transcrit un fichier — sur la carte graphique comme Eve,
//!   si son serveur est là ; `eve --essai-oreille dossier [modèle]` : chacun de ses WAV, sur le processeur ;
//! - `eve --essai-micro [secondes]` : le micro écoute, puis le bilan de ses paquets — perd-il du son ? ;
//! - `eve --essai-cartes [dossier] [scène]` : les cartes de la fenêtre native, dans une fenêtre à part ;
//! - `eve --essai-musique [secondes] [dossier] [--marge]` : la musique du centre de commande, et ce qu'en entend le
//!   micro ;
//! - `eve --essai-claude question…` : un tour complet avec l'Adjointe, la garde branchée — et tout ce qui
//!   demanderait l'accord est refusé d'office (l'essai n'écrit jamais rien) ;
//! - `eve --essai-orbe [--etat NOM] [--mesure]` : l'orbe de la fenêtre native, seule, d'un état à l'autre ;
//! - `eve --essai-meteo [latitude longitude]` : une lecture de la météo du centre de commande (Montréal d'office), et
//!   la tempête qu'elle donnerait en hiver ;
//! - `eve --essai-reseau [scène] [--mesure]` : le réseau des actions plein fenêtre, sa démonstration, ses images ;
//! - `eve --essai-panneaux [dossier] [heure] [--comptabilite]` : les panneaux du centre de commande, sur des données
//!   inventées, à une heure dite ; l'argent d'une comptabilité inventée, comme le réglage `comptabilite` ;
//! - `eve --essai-ambiance [scène] [--mesure]` : l'ambiance du centre de commande (la saison, la fête, le balayage, la
//!   nuit) et son écran de veille ;
//! - `eve --essai-historique [dossier] [--captures DOSSIER]` : le panneau de l'historique, sur des conversations
//!   inventées ;
//! - `eve --essai-constructo [port] [--duree s] [--outils N] [--reveil s]` : un faux Constructo AI sur le poste (OAuth
//!   et MCP), pour essayer la connexion d'Eve sans jamais toucher au vrai ;
//! - `eve --essai-cadre [--captures DOSSIER]` : la barre de titre d'Eve, sur la vraie fenêtre, mise à l'épreuve comme
//!   Windows le ferait ;
//! - `eve --essai-theme [dossier] [--captures DOSSIER] [--scenes A,B…]` : la fenêtre native dans le thème « Constructo
//!   AI », sur un état, des panneaux et une entreprise inventés, ses scènes capturées ;
//! - `eve --essai-pieces [--captures DOSSIER] [--sans-selecteur]` : les pièces jointes de la fenêtre native, déposées,
//!   refusées, retirées, choisies par le trombone, sur des fichiers inventés ; et celle de la page ;
//! - `eve --essai-profils [--captures DOSSIER]` : le menu des profils de la barre et la rubrique « Profil » des
//!   réglages, cliqués sans la souris de la personne, sur des profils inventés ;
//! - `eve --essai-portrait <dossier> [--captures DOSSIER]` : le portrait d'Eve d'un dossier local dans toutes ses
//!   poses, côte à côte ;
//! - `eve --essai-services [--nombres] [--suivre SECONDES]` : une lecture des services en ligne du tableau de bord —
//!   les pages d'état de Claude, de GitHub et de Render, les erreurs des ressources de Render —, imprimée ; le suivi
//!   qui continue, une clé retirée ou posée en marche.
//! - `eve --essai-stripe` : une lecture du compte Stripe, avec la clé du dossier local, imprimée comme le tableau de
//!   bord la montrerait ; les routes lues et les droits d'une clé restreinte.
//! - `eve --essai-son textes.txt dossier` : ce qu'Eve dirait de chaque texte, rendu sans le jouer — les WAV que
//!   recevrait le lecteur, et leur tout.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;
use eve::garde::{self, Decision};
use eve::parole::{self, EnFlux};
use eve::protocole::{self, Flux, Recu};
use serde_json::Value;

use crate::cerveau::{self, Cerveau, Lancement};
use crate::chef::Evt;
use crate::natif::couleurs;
use crate::orbe::Orbe;
use crate::reglages::Reglages;
use crate::{micro, oreille, voix};

pub fn lister_voix() -> i32 {
    voix::initialiser_winrt();
    for v in voix::lister() {
        println!("{:<40} {}", v.id, v.nom);
    }
    0
}

pub fn voix(r: &Reglages, args: &[String]) -> i32 {
    let mut reglages = r.clone();
    let mut mots = args.to_vec();
    if mots.first().map(String::as_str) == Some("--id") && mots.len() > 1 {
        reglages.voix = mots[1].clone();
        mots.drain(..2);
    }
    let texte = if mots.is_empty() { "Bonjour, je suis Eve, ton adjointe.".to_string() } else { mots.join(" ") };
    let (tx, rx) = mpsc::channel();
    let v = voix::demarrer(&reglages, tx);
    let t0 = Instant::now();
    v.dire(1, &texte);
    for evt in rx {
        match evt {
            Evt::Voix { choisie, .. } => println!("voix : {choisie}"),
            Evt::VoixErreur(e) => {
                eprintln!("erreur : {e}");
                return 1;
            }
            Evt::VoixFinie(1) => {
                println!("dit en {:.1} s", t0.elapsed().as_secs_f32());
                return 0;
            }
            _ => {}
        }
    }
    1
}

/// `eve --essai-micro [secondes]` : le micro par défaut, écouté ce temps (30 s d'office) sans rien en faire, puis le
/// bilan de ses paquets. Chacun dit quand son premier échantillon a été capté (la position QPC que rend WASAPI) : un
/// écart entre deux paquets plus long que la durée du premier est un trou dans le son. On sait ainsi si les
/// « accrocs » du journal (le drapeau de discontinuité de WASAPI) font perdre du son, ou ne sont qu'un signal de
/// l'appareil.
pub fn micro(args: &[String]) -> i32 {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let secondes = args.first().and_then(|s| s.parse::<u64>().ok()).unwrap_or(30).clamp(5, 600);
    let Some(appareil) = cpal::default_host().default_input_device() else {
        eprintln!("aucun micro branché");
        return 1;
    };
    let nom = appareil.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "micro".into());
    let cfg = match appareil.default_input_config() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("le micro ne dit pas son format : {e}");
            return 1;
        }
    };
    let (taux, canaux, format) = (cfg.sample_rate(), usize::from(cfg.channels()).max(1), cfg.sample_format());
    println!("micro : {nom} — {taux} Hz, {canaux} canal(aux), {format:?} ; {secondes} s d'écoute");
    // Chaque paquet : l'instant de capture de son premier échantillon, et ses trames ; chaque accroc : son instant.
    let paquets: Arc<Mutex<Vec<(cpal::StreamInstant, usize)>>> = Arc::default();
    let accrocs: Arc<Mutex<Vec<Instant>>> = Arc::default();
    let (p, a) = (paquets.clone(), accrocs.clone());
    let flux = appareil.build_input_stream_raw(
        cfg.config(),
        format,
        move |donnees: &cpal::Data, info: &cpal::InputCallbackInfo| {
            if let Ok(mut v) = p.lock() {
                v.push((info.timestamp().capture, donnees.len() / canaux));
            }
        },
        move |e: cpal::Error| {
            if matches!(e.kind(), cpal::ErrorKind::Xrun)
                && let Ok(mut v) = a.lock()
            {
                v.push(Instant::now());
            } else {
                eprintln!("micro : {e}");
            }
        },
        None,
    );
    let flux = match flux.map_err(|e| e.to_string()).and_then(|f| f.play().map(|()| f).map_err(|e| e.to_string())) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("le micro ne s'ouvre pas : {e}");
            return 1;
        }
    };
    let debut = Instant::now();
    thread::sleep(Duration::from_secs(secondes));
    drop(flux);
    let paquets = paquets.lock().map(|v| v.clone()).unwrap_or_default();
    let accrocs = accrocs.lock().map(|v| v.clone()).unwrap_or_default();
    // La première seconde, le flux s'installe : elle ne compte pas.
    let Some(&(premier, _)) = paquets.first() else {
        eprintln!("aucun paquet reçu en {secondes} s : le micro ne livre rien");
        return 1;
    };
    let installe = premier.checked_add(Duration::from_secs(1)).unwrap_or(premier);
    let v: Vec<(cpal::StreamInstant, usize)> = paquets.into_iter().filter(|(t, _)| *t >= installe).collect();
    let accrocs = accrocs.iter().filter(|t| t.duration_since(debut) >= Duration::from_secs(1)).count();
    if v.len() < 2 {
        eprintln!("trop peu de paquets pour un bilan");
        return 1;
    }
    let duree = |trames: usize| Duration::from_secs_f64(trames as f64 / f64::from(taux));
    let mut trous: Vec<Duration> = Vec::new();
    for paire in v.windows(2) {
        let ((t0, n0), (t1, _)) = (paire[0], paire[1]);
        let attendu = duree(n0);
        let ecart = t1.duration_since(t0);
        // Une tolérance : la position QPC d'un paquet n'est pas à la microseconde près.
        if ecart > attendu + (attendu / 4).max(Duration::from_millis(2)) {
            trous.push(ecart - attendu);
        }
    }
    let (dernier, n_dernier) = v[v.len() - 1];
    let etendue = dernier.duration_since(v[0].0) + duree(n_dernier);
    let recues: usize = v.iter().map(|(_, n)| n).sum();
    let attendues = etendue.as_secs_f64() * f64::from(taux);
    let perdu: Duration = trous.iter().sum();
    let plus_long = trous.iter().max().copied().unwrap_or_default();
    println!(
        "{} paquets en {:.1} s (la première seconde mise à part) : {recues} trames reçues pour {attendues:.0} attendues",
        v.len(),
        etendue.as_secs_f64()
    );
    println!(
        "trous dans le son : {}, {} ms en tout (le plus long : {} ms) ; accrocs signalés : {accrocs}",
        trous.len(),
        perdu.as_millis(),
        plus_long.as_millis()
    );
    let part = perdu.as_secs_f64() / etendue.as_secs_f64() * 100.0;
    if trous.is_empty() {
        println!("→ aucun trou : les accrocs ne font pas perdre de son.");
    } else {
        println!("→ le son a des trous : {part:.2} % du temps.");
    }
    0
}

pub fn oreille(r: &Reglages, args: &[String]) -> i32 {
    let Some(fichier) = args.first() else {
        eprintln!("usage : eve --essai-oreille fichier.wav");
        return 2;
    };
    if Path::new(fichier).is_dir() {
        return oreille_d_un_dossier(r, Path::new(fichier), args.get(1));
    }
    let audio = match micro::lire_wav(Path::new(fichier)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    println!("son : {:.1} s à 16 kHz", audio.len() as f32 / micro::CADENCE as f32);
    // Un second argument : un autre modèle (pour comparer les durées).
    let modele = args.get(1).map(std::path::PathBuf::from).unwrap_or_else(|| r.chemin_oreille());
    // Sur la carte graphique, comme Eve, si son serveur est là (`EVE_SANS_CARTE` : le processeur) : trois passes, la
    // première paie l'échauffement du modèle.
    let serveur = crate::reglages::ressource("whisper-gpu").join("whisper-server.exe");
    if serveur.is_file() && std::env::var_os("EVE_SANS_CARTE").is_none() {
        let t0 = Instant::now();
        let mut carte = match oreille::Carte::lancer(&serveur, &modele) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("la carte graphique ne démarre pas : {e}");
                return 1;
            }
        };
        println!("carte graphique : {} — {} ({:.1} s)", modele.display(), carte.resume(), t0.elapsed().as_secs_f32());
        for passe in 1..=3 {
            let t0 = Instant::now();
            match carte.transcrire(&audio, &r.vocabulaire_complet()) {
                Ok(texte) => println!("passe {passe} : {:.2} s — {texte}", t0.elapsed().as_secs_f32()),
                Err(e) => {
                    eprintln!("passe {passe} : {e}");
                    return 1;
                }
            }
        }
        return 0;
    }
    let t0 = Instant::now();
    let contexte =
        match whisper_rs::WhisperContext::new_with_params(&modele, whisper_rs::WhisperContextParameters::default()) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("le modèle ne se charge pas : {e}");
                return 1;
            }
        };
    let Ok(mut etat) = contexte.create_state() else {
        eprintln!("Whisper ne démarre pas");
        return 1;
    };
    println!("modèle chargé en {:.1} s : {}", t0.elapsed().as_secs_f32(), modele.display());
    // Un troisième argument : le nombre de fils de calcul (pour comparer).
    let fils = args.get(2).and_then(|n| n.parse().ok()).unwrap_or_else(oreille::fils_de_calcul);
    println!("fils de calcul : {fils}");
    // L'essai des dénormaux : EVE_FTZ=1 règle ce fil pour les arrondir à zéro (FTZ et DAZ du registre MXCSR).
    if std::env::var("EVE_FTZ").is_ok() {
        crate::oreille::arrondir_les_denormaux();
        println!("dénormaux arrondis à zéro sur ce fil");
    }
    // Les étapes une à une : le spectre, puis l'encodeur (la plus lourde, sur une fenêtre de 30 s).
    let t0 = Instant::now();
    if let Err(e) = etat.pcm_to_mel(&audio, fils as usize) {
        eprintln!("spectre : {e}");
    }
    println!("spectre : {:.2} s", t0.elapsed().as_secs_f32());
    let t0 = Instant::now();
    if let Err(e) = etat.encode(0, fils as usize) {
        eprintln!("encodeur : {e}");
    }
    println!("encodeur : {:.2} s", t0.elapsed().as_secs_f32());
    let t0 = Instant::now();
    let texte = oreille::transcrire(&mut etat, &audio, &r.vocabulaire_complet(), fils);
    println!("transcrit en {:.1} s : {texte}", t0.elapsed().as_secs_f32());
    println!("nettoyé : {}", parole::nettoyer_transcription(&texte));
    // Le détail de whisper.cpp (sur la sortie d'erreur) : le spectre, l'encodeur, le décodeur.
    contexte.print_timings();
    0
}

pub fn claude(r: &Reglages, args: &[String]) -> i32 {
    // Plusieurs questions, séparées par « || », se posent l'une après l'autre dans la même séance : la seconde montre
    // le délai au chaud, le contexte de l'Adjointe déjà en cache.
    let texte =
        if args.is_empty() { "Quelle date sommes-nous ? Réponds en une phrase.".to_string() } else { args.join(" ") };
    let mut questions: VecDeque<String> =
        texte.split("||").map(|q| q.trim().to_string()).filter(|q| !q.is_empty()).collect();
    let (Some(dossier), Some(exe)) = (r.dossier_adjointe(), cerveau::trouver_claude()) else {
        eprintln!("dossier de l'Adjointe ou Claude Code introuvable");
        return 1;
    };
    eve::garde::reconnaitre_dossier(&dossier.to_string_lossy());
    let entreprise = match r.nom_entreprise() {
        "" => cerveau::nom_entreprise(&dossier).unwrap_or_default(),
        nom => nom.to_string(),
    };
    let (tx, rx) = mpsc::channel();
    let l = Lancement {
        claude: exe,
        dossier: dossier.clone(),
        dossier_d_eve: dossier.clone(),
        profil: String::new(),
        modele: r.modele.clone(),
        effort: r.effort.clone(),
        qui: r.qui.clone(),
        entreprise,
        jeton_mcp: cerveau::jeton_mcp(&dossier),
        reprendre: None,
        generation: 1,
        espace: r.espace(crate::reglages::est_un_specialiste(&dossier)),
    };
    println!(
        "dossier : {} ; jeton de la base : {}",
        dossier.display(),
        if l.jeton_mcp.is_some() { "oui" } else { "non" }
    );
    let c = match Cerveau::lancer(&l, tx) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Claude Code ne démarre pas : {e}");
            return 1;
        }
    };
    // EVE_AVEC_CALENDRIER : une question de calendrier porte les quinze jours, comme dans Eve.
    let calendrier = if std::env::var("EVE_AVEC_CALENDRIER").is_ok() {
        l.jeton_mcp.as_deref().and_then(|j| crate::veilleur::lire_les_jours(j).ok())
    } else {
        None
    };
    let avec_calendrier = |q: &str| match &calendrier {
        Some((lu, jours)) if parole::parle_du_calendrier(q) => {
            format!("{q}{}", crate::chef::bloc_calendrier(lu, jours))
        }
        _ => q.to_string(),
    };
    let t0 = Instant::now();
    let mut crochets = 0u32;
    // La réponse en flux : ce que la voix dirait, phrase par phrase, avec l'instant où chaque phrase est prête.
    let mut flux = EnFlux::default();
    let mut envoi: Option<Instant> = None;
    let mut premiere: Option<f32> = None;
    // Claude Code rend le cumul de sa séance : le coût d'une question en est la différence.
    let mut cumul: Option<f64> = None;
    for evt in rx {
        let message = match evt {
            Evt::Claude { message, .. } => message,
            Evt::ClaudeFini { .. } => {
                eprintln!("Claude Code s'est arrêté avant la fin du tour");
                return 1;
            }
            _ => continue,
        };
        match protocole::lire(&message) {
            Recu::Ouverture { modele, serveurs, outils, .. } => {
                println!("ouvert : {modele} ; serveurs MCP : {serveurs:?}\noutils offerts : {}", outils.join(", "))
            }
            Recu::Reponse { id, reussie, erreur, .. } if id == c.id_init => {
                if !reussie {
                    eprintln!("initialisation refusée : {erreur}");
                    return 1;
                }
                println!("initialisation acceptée : la garde est branchée ({:.1} s)", t0.elapsed().as_secs_f32());
                let question = questions.pop_front().unwrap_or_default();
                println!("question : {question}");
                if let Err(e) = c.dire(&avec_calendrier(&question)) {
                    eprintln!("{e}");
                    return 1;
                }
                envoi = Some(Instant::now());
            }
            Recu::Flux(f) => {
                let phrases = match f {
                    Flux::Debut => {
                        flux.nouveau_message();
                        Vec::new()
                    }
                    Flux::Texte(t) => flux.ajouter(&t),
                    Flux::FinBloc => flux.finir(),
                    Flux::DebutTexte | Flux::Outil(_) => Vec::new(),
                };
                for p in phrases {
                    let s = envoi.map(|e| e.elapsed().as_secs_f32()).unwrap_or(0.0);
                    premiere.get_or_insert(s);
                    println!("  [voix, à {s:.1} s] {p}");
                }
            }
            Recu::Adjointe { outils, .. } => {
                for (_, nom, entree) in outils {
                    println!("{}", protocole::resume_outil(&nom, entree));
                }
            }
            Recu::Demande { id, sorte, corps } => {
                let (nom, entree) = match sorte.as_str() {
                    "hook_callback" => (corps.pointer("/input/tool_name"), corps.pointer("/input/tool_input")),
                    _ => (corps.get("tool_name"), corps.get("input")),
                };
                crochets += 1;
                let nom = nom.and_then(Value::as_str).unwrap_or("");
                let entree = entree.cloned().unwrap_or(Value::Null);
                let refus = |raison: &str| {
                    if sorte == "hook_callback" {
                        protocole::crochet_refus(raison)
                    } else {
                        protocole::permission_refus(raison)
                    }
                };
                let decision = match l.espace.refus(nom) {
                    Some(raison) => Decision::Refuser { raison },
                    None => garde::examiner(nom, &entree),
                };
                let corps = match decision {
                    Decision::Libre if sorte == "hook_callback" => protocole::crochet_neutre(),
                    Decision::Libre => protocole::permission_accord(&entree),
                    Decision::Refuser { raison } => {
                        println!("  garde : refusé d'office — {raison}");
                        refus(&raison)
                    }
                    Decision::Confirmer { quoi, detail } => {
                        println!("  garde : ACCORD DEMANDÉ — {quoi}\n    {}", garde::ecourter(&detail, 200));
                        refus("Essai d'Eve : toute écriture est refusée d'office.")
                    }
                };
                let _ = c.repondre(&id, corps);
            }
            Recu::Fin { texte, erreur, cout, duree_ms } => {
                println!(
                    "--- fin du tour : {:.1} s ({} ms chez Claude), {} crochet(s), {} ---",
                    t0.elapsed().as_secs_f32(),
                    duree_ms.unwrap_or(0),
                    crochets,
                    cout.map(|c| format!("{:.4} $ (cumul {c:.4} $)", eve::mesures::cout_du_tour(cumul.replace(c), c)))
                        .unwrap_or_default()
                );
                println!("{texte}");
                println!("--- à voix haute ---\n{}", parole::pour_la_voix(&texte));
                if let Some(p) = premiere.take() {
                    println!(
                        "--- première phrase prête à {p:.1} s (la réponse entière à {:.1} s) ---",
                        envoi.map(|e| e.elapsed().as_secs_f32()).unwrap_or(0.0)
                    );
                }
                if erreur {
                    return 1;
                }
                if let Some(question) = questions.pop_front() {
                    flux.nouveau_tour();
                    println!("question : {question}");
                    if let Err(e) = c.dire(&avec_calendrier(&question)) {
                        eprintln!("{e}");
                        return 1;
                    }
                    envoi = Some(Instant::now());
                    continue;
                }
                return 0;
            }
            _ => {}
        }
    }
    1
}

/// Les états que parcourt `eve --essai-orbe`, dans l'ordre d'une demande.
const ORDRE_DE_L_ORBE: [&str; 8] =
    ["demarrage", "pret", "ecoute", "transcription", "reflexion", "parole", "confirmation", "erreur"];

/// `eve --essai-orbe [--etat NOM] [--mesure] [--repli] [--clair] [--secondes N]` : l'orbe seule, dans une fenêtre aux
/// couleurs de la fenêtre native (son thème sombre ; `--clair` : le thème clair de la page, et le studio plus clair que
/// le verre renvoie alors) ; elle passe d'un état à l'autre toutes les trois secondes, ou reste sur NOM. En
/// confirmation, elle se réduit à 112 points, comme sous la carte d'accord ; en écoute, une voix simulée la fait vivre
/// (celle de la page de démonstration). `--mesure` relève, état par état, la cadence et ce que coûte une image (sa
/// préparation sur le processeur, ses trois passes sur la carte graphique), et l'écrit en partant ; `--repli` montre le
/// disque qui la remplace sans OpenGL ; `--secondes N` ferme la fenêtre d'elle-même.
pub fn orbe(args: &[String]) -> i32 {
    let valeur = |nom: &str| args.iter().position(|a| a == nom).and_then(|i| args.get(i + 1)).cloned();
    let fixe = valeur("--etat");
    let secondes = valeur("--secondes").and_then(|s| s.parse::<f64>().ok());
    let drapeau = |nom: &str| args.iter().any(|a| a == nom);
    let (mesure, repli, clair) = (drapeau("--mesure"), drapeau("--repli"), drapeau("--clair"));
    let titre = "Eve — essai de l'orbe";
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title(titre).with_inner_size([360.0, 400.0]),
        ..Default::default()
    };
    let creer: eframe::AppCreator<'_> = Box::new(move |cc| {
        // Le thème de la fenêtre native (sombre), que l'orbe lit pour son studio — ou le clair de la page.
        cc.egui_ctx.set_theme(if clair { egui::Theme::Light } else { egui::Theme::Dark });
        let mut orbe = Orbe::nouvelle();
        if mesure {
            orbe.mesurer();
        }
        if repli {
            orbe.sans_opengl();
        }
        Ok(Box::new(EssaiOrbe {
            orbe,
            fixe,
            secondes,
            mesure,
            images: VecDeque::new(),
            releves: Vec::new(),
            avant: None,
        }))
    });
    match eframe::run_native(titre, options, creer) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("la fenêtre ne s'ouvre pas : {e}");
            1
        }
    }
}

/// Ce que `--mesure` relève dans un état : le temps qu'il a duré, ses images, ce que chacune a coûté (en ms).
struct Releve {
    etat: String,
    duree: f64,
    images: u32,
    preparation: Vec<f64>,
    carte: Vec<f64>,
}

struct EssaiOrbe {
    orbe: Orbe,
    fixe: Option<String>,
    secondes: Option<f64>,
    mesure: bool,
    /// Les instants des images des deux dernières secondes : la cadence affichée.
    images: VecDeque<f64>,
    releves: Vec<Releve>,
    /// L'instant de l'image précédente.
    avant: Option<f64>,
}

impl EssaiOrbe {
    fn relever(&mut self, etat: &str, t: f64) {
        let dt = self.avant.map_or(0.0, |a| t - a);
        self.avant = Some(t);
        let (preparation, carte) = self.orbe.mesures();
        let i = match self.releves.iter().position(|r| r.etat == etat) {
            Some(i) => i,
            None => {
                let r = Releve { etat: etat.to_string(), duree: 0.0, images: 0, preparation: vec![], carte: vec![] };
                self.releves.push(r);
                self.releves.len() - 1
            }
        };
        let r = &mut self.releves[i];
        r.duree += dt;
        r.images += 1;
        r.preparation.push(preparation);
        r.carte.extend(carte);
    }
}

fn mediane(v: &[f64]) -> Option<f64> {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied()
}

impl eframe::App for EssaiOrbe {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let t = ui.input(|i| i.time);
        if self.secondes.is_some_and(|s| t >= s) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let etat = self.fixe.clone().unwrap_or_else(|| ORDRE_DE_L_ORBE[(t / 3.0) as usize % 8].to_string());
        // La voix de la page de démonstration : 0,25 + 0,6 |sin(t / 170 ms)| × le hasard.
        let hasard = ((t * 12.9898).sin() * 43_758.545).rem_euclid(1.0);
        let niveau =
            if etat == "ecoute" { (0.25 + 0.6 * (t * 1000.0 / 170.0).sin().abs() * hasard) as f32 } else { 0.0 };
        self.images.push_back(t);
        while self.images.front().is_some_and(|x| t - x > 2.0) {
            self.images.pop_front();
        }
        let cadence = match self.images.front() {
            Some(debut) if t > *debut => (self.images.len() - 1) as f64 / (t - debut),
            _ => 0.0,
        };
        let (orbe, mesure) = (&mut self.orbe, self.mesure);
        // Le fond de la conversation : la surface sombre de la fenêtre native, ou le fond bleu pâle de la page claire.
        let sombre = ui.visuals().dark_mode;
        let fond = if sombre { couleurs::SURFACE } else { egui::Color32::from_rgb(0xEE, 0xF4, 0xFB) };
        let (encre, encre_3) = if sombre {
            (couleurs::ENCRE, couleurs::ENCRE_3)
        } else {
            (egui::Color32::from_rgb(0x1F, 0x29, 0x37), egui::Color32::from_rgb(0x5F, 0x6B, 0x7A))
        };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(fond)).show(ui, |ui| {
            let zone = ui.max_rect();
            let cible = if etat == "confirmation" { 112.0 } else { 236.0 };
            let cote = ui.ctx().animate_value_with_time(egui::Id::unique("essai-orbe"), cible, 0.35);
            let centre = egui::pos2(zone.center().x, zone.top() + 24.0 + 118.0);
            let rect = egui::Rect::from_center_size(centre, egui::vec2(cote, cote));
            let anime = orbe.dessiner(ui, rect, &etat, niveau, t);
            let p = ui.painter();
            let x = zone.center().x;
            let y = zone.top() + 24.0 + 236.0 + 16.0;
            let ecrire = |y: f32, texte: String, taille: f32, couleur| {
                p.text(egui::pos2(x, y), egui::Align2::CENTER_TOP, texte, egui::FontId::proportional(taille), couleur);
            };
            ecrire(y, etat.clone(), 19.0, encre);
            let rythme = if anime { "elle s'anime" } else { "au repos" };
            ecrire(y + 30.0, format!("{rythme} — {cadence:.0} images par seconde"), 13.0, encre_3);
            if mesure {
                let (preparation, carte) = orbe.mesures();
                let carte = carte.map_or("?".to_string(), |c| format!("{c:.2} ms"));
                ecrire(y + 50.0, format!("préparation {preparation:.2} ms · carte graphique {carte}"), 12.0, encre_3);
            }
        });
        if self.mesure {
            self.relever(&etat, t);
        }
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        if let Some(gl) = gl {
            self.orbe.liberer(gl);
        }
        if self.mesure {
            println!("état            images/s   préparation   carte graphique (médianes, par image)");
            for r in &self.releves {
                let ms = |v: Option<f64>| v.map_or("?".to_string(), |v| format!("{v:.3} ms"));
                let cadence = if r.duree > 0.0 { f64::from(r.images) / r.duree } else { 0.0 };
                println!(
                    "{:<15} {cadence:>8.1}   {:>11}   {:>15}",
                    r.etat,
                    ms(mediane(&r.preparation)),
                    ms(mediane(&r.carte))
                );
            }
        }
    }
}

#[cfg(test)]
mod en_ligne {
    /// Un vrai tour avec l'Adjointe (Claude Code), la parole en flux affichée — toute écriture refusée d'office :
    /// `cargo test --release -- --ignored --nocapture un_tour_avec_l_adjointe` (la question : EVE_QUESTION).
    #[test]
    #[ignore]
    fn un_tour_avec_l_adjointe() {
        let mut r = crate::reglages::charger();
        // Pour comparer les délais : EVE_EFFORT=low|medium|high, EVE_MODELE=…
        if let Ok(e) = std::env::var("EVE_EFFORT") {
            r.effort = e;
        }
        if let Ok(m) = std::env::var("EVE_MODELE") {
            r.modele = m;
        }
        let q = std::env::var("EVE_QUESTION")
            .unwrap_or_else(|_| "Qu'est-ce que j'ai au calendrier demain ? Réponds en deux phrases.".into());
        assert_eq!(super::claude(&r, &[q]), 0);
    }
}

// ---- Les cartes de la fenêtre native (le lot 6.3) -----------------------------------------------------------------

/// `eve --essai-cartes [dossier] [scène]` : une fenêtre au style de la fenêtre native, et des bulles d'Eve qui portent
/// chaque sorte de carte, dans une colonne étroite (la conversation à côté du centre de commande) et une large (le
/// centre replié). Le dossier d'essai sert de dossier de travail : ses aperçus s'affichent (image.png, photo.jpg,
/// anime.gif, image.webp, plan.svg, soumission.pdf, page.html ; gros.png est trop gros) ; les autres chemins se
/// refusent (absent, hors du dossier, réseau, « .. », la jonction `lien`, relatif). Les scènes : tout (d'office),
/// agenda, tableau, brouillon, apercus (les images), plans (le SVG, le PDF), pages (la page HTML, le WebP, le GIF),
/// refus et chemins (ce qui ne s'affiche pas), dessins (les blocs « ```svg » d'Eve), divers. Un clic sur une carte
/// affiche le geste qu'elle rend, et rien d'autre : l'essai n'a pas de chef (« Enregistrer » un dessin ouvre la vraie
/// boîte de Windows).
pub fn cartes(args: &[String]) -> i32 {
    use eframe::egui;
    let dossier =
        args.first().map(std::path::PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("eve-essai-cartes"));
    let scene = args.get(1).map_or("tout", String::as_str).to_string();
    let exemples: Vec<ExempleDeCarte> = exemples_de_cartes(&dossier)
        .into_iter()
        .filter(|e| scene == "tout" || e.scene.split(' ').any(|s| s == scene))
        .collect();
    if exemples.is_empty() {
        let scenes =
            "tout, vitrine, agenda, tableau, brouillon, apercus, plans, pages, refus, chemins, dessins, divers";
        eprintln!("scène inconnue : {scene} ({scenes})");
        return 2;
    }
    let titre = "Eve — essai des cartes";
    let vue = egui::ViewportBuilder::default()
        .with_title(titre)
        .with_inner_size([1250.0, 980.0])
        .with_min_inner_size([900.0, 400.0]);
    let options = eframe::NativeOptions { viewport: vue, ..Default::default() };
    let local = crate::reglages::dossier_local();
    let creer: eframe::AppCreator<'_> = Box::new(move |cc| {
        crate::natif::polices(&cc.egui_ctx);
        crate::natif::apparence(&cc.egui_ctx);
        let cartes = crate::cartes::Cartes::nouvelles(Some(dossier), local);
        Ok(Box::new(EssaiDesCartes { cartes, exemples, geste: String::new() }))
    });
    match eframe::run_native(titre, options, creer) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("la fenêtre ne s'ouvre pas : {e}");
            1
        }
    }
}

/// Une bulle de l'essai : sa scène, la phrase qui précède la carte, la carte (son JSON, ou le SVG d'un dessin), Eve
/// l'écrit-elle encore, son opacité.
struct ExempleDeCarte {
    scene: &'static str,
    avant: &'static str,
    contenu: String,
    dessin: bool,
    en_cours: bool,
    opacite: f32,
}

/// Les exemples du § 2.6 de `docs\fenetre-native.md`, et de quoi éprouver chaque règle : un tableau long et large, un
/// agenda qui a toutes les pastilles, un brouillon à l'extrait trop long, les cartes illisibles, une carte qui
/// s'écrit, une ligne d'une séance précédente ; les aperçus des fichiers du dossier d'essai, et les chemins refusés.
fn exemples_de_cartes(dossier: &std::path::Path) -> Vec<ExempleDeCarte> {
    use serde_json::json;
    let f = |nom: &str| dossier.join(nom).display().to_string();
    let apercu =
        |titre: &str, chemin: String| json!({ "type": "apercu", "titre": titre, "chemin": chemin }).to_string();
    let ex = |scene, avant, contenu: String| ExempleDeCarte {
        scene,
        avant,
        contenu,
        dessin: false,
        en_cours: false,
        opacite: 1.0,
    };
    let dessin = |scene, avant, svg: &str, en_cours| ExempleDeCarte {
        scene,
        avant,
        contenu: svg.to_string(),
        dessin: true,
        en_cours,
        opacite: 1.0,
    };
    let factures = json!({
        "type": "tableau",
        "titre": "Factures à vérifier",
        "colonnes": ["Fournisseur", "Nº", "Total"],
        "lignes": [["Matériaux Exemple", "9013623395", "31,08 $"], ["Plomberie Exemple", "96048", "1\u{a0}204,50 $"]],
        "pied": ["Total", "", "1\u{a0}235,58 $"]
    })
    .to_string();
    let fournisseurs =
        ["Matériaux Exemple", "Plomberie Exemple inc.", "Électricité du Nord", "Location d'outils Saint-Jérôme"];
    let articles = [
        "Bois d'œuvre, 2 × 4 × 8 pi, épinette no 2",
        "Raccords PEX ½ po",
        "Fil 14/2 NMD90, 75 m",
        "Échafaudage, une semaine",
    ];
    let lignes: Vec<serde_json::Value> = (1..=28u32)
        .map(|i| {
            let k = i as usize % 4;
            json!([
                format!("2026-10-{:02}", i % 28 + 1),
                fournisseurs[k],
                (9_013_623_000u64 + u64::from(i) * 7919).to_string(),
                articles[k],
                format!("{} h", (i * 3) % 9 + 1),
                format!("{} %", (i * 7) % 15),
                format!("{}\u{a0}{:03},{:02} $", (i * 37) % 9 + 1, (i * 137) % 1000, (i * 37) % 100),
                if i % 5 == 0 { "(45,00 $)".to_string() } else { format!("{} km", i * 3) },
            ])
        })
        .collect();
    let long = json!({
        "type": "tableau",
        "titre": "Achats d'octobre, chantier des Érables",
        "colonnes": ["Date", "Fournisseur", "Nº de facture", "Article", "Durée", "Escompte", "Montant", "Trajet"],
        "lignes": lignes,
        "pied": [
            ["Sous-total", "", "", "", "", "", "70\u{a0}419,32 $", ""],
            ["Total, taxes comprises", "", "", "", "", "", "80\u{a0}964,59 $", "1\u{a0}218 km"]
        ]
    })
    .to_string();
    let demain = json!({
        "type": "agenda",
        "titre": "Demain, jeudi 22 octobre",
        "jours": [{ "jour": "jeu. 22 oct.", "evenements": [
            { "heure": "7 h 30", "titre": "Démolition des jupettes", "detail": "Chalet Exemple", "sorte": "confirme" },
            { "heure": "16 h", "titre": "Remettre la soumission", "detail": "Échéance du fournisseur",
              "sorte": "echeance" }
        ] }]
    })
    .to_string();
    let semaine = json!({
        "type": "agenda",
        "titre": "La semaine du chantier",
        "jours": [
            { "jour": "lun. 26 oct.", "evenements": [
                { "heure": "7 h", "titre": "Livraison du bois", "detail": "Chantier des Érables, 12 rue des Érables",
                  "sorte": "confirme" },
                { "heure": "10 h 30", "titre": "Visite du client", "detail": "À confirmer avec Julie",
                  "sorte": "attente" },
                { "heure": "13 h", "titre": "Appeler l'inspecteur municipal", "sorte": "appel" },
                { "heure": "16 h", "titre": "Remettre la soumission", "detail": "Échéance du fournisseur",
                  "sorte": "echeance" }
            ] },
            { "jour": "mar. 27 oct.", "evenements": [
                { "heure": "8 h", "titre": "Coulée de la dalle du garage", "detail": "Le jalon de la semaine",
                  "sorte": "gros" },
                { "heure": "9 h", "titre": "Formation SST, toute la matinée", "detail": "Sans sorte : un carré vide" },
                "Rappeler Marc (un événement qui n'est qu'un titre)"
            ] },
            { "jour": "mer. 28 oct.", "evenements": [] },
            { "jour": "jeu. 29 oct.", "evenements": [{ "titre": "Journée fériée (exemple)", "sorte": "ferie" }] }
        ]
    })
    .to_string();
    let brouillon = json!({
        "type": "brouillon",
        "a": ["Julie (Poutrelles Exemple)", "Marc Tremblay"],
        "objet": "020 — Approbation du plan des poutrelles",
        "extrait": "Bonjour Julie,\nLe plan est approuvé, sous réserve de la hauteur des solives au-dessus du \
                    garage.\nPeux-tu me confirmer la date de livraison ?\nMerci,\nSam\nCes deux lignes-ci\nne \
                    s'affichent pas.",
        "id": "00000000A1B2C3D4E5F60718293A4B5C"
    })
    .to_string();
    // La carte de la capture du 24 octobre : sa note au pied s'étirait dans la colonne « Jour ».
    let meteo = json!({
        "type": "tableau",
        "titre": "Météo des trois jours, 24 au 26 octobre 2026",
        "colonnes": ["Jour", "Min / max", "Conditions", "Précipitations"],
        "lignes": [
            ["Samedi 24", "0 / 11 °C", "Ensoleillé", "0 mm"],
            ["Dimanche 25", "3 / 14 °C", "Soleil, doux", "0 mm"],
            ["Lundi 26", "3 / 10 °C", "Nuageux, bruine l'après-midi", "1 mm"]
        ],
        "pied": ["Prévisions de wttr.in, consultées le samedi 24 octobre 2026. Au-delà du lundi 26, prévision non trouvée."]
    })
    .to_string();
    let inconnue = json!({ "type": "graphique", "titre": "Ventes par mois", "donnees": [12, 18, 9] }).to_string();
    let illisible = r#"{"type":"tableau","colonnes":["A","B"],"lignes":[["1","2"]"#.to_string();
    let fond_d_ecran = r"C:\Windows\Web\Wallpaper\Windows\img0.jpg".to_string();
    vec![
        ex("agenda vitrine", "Voici ta journée de demain.", demain),
        ex("agenda", "Toutes les sortes de la semaine.", semaine),
        ex("tableau vitrine", "Deux factures attendent.", factures.clone()),
        ex("tableau", "La météo des trois jours.", meteo),
        ex("tableau", "Le détail des achats.", long),
        ex("brouillon vitrine", "Le brouillon est prêt.", brouillon),
        ex("apercus", "Le graphique des ventes.", apercu("Graphique des ventes", f("image.png"))),
        ex("apercus", "La photo, dans son bon sens.", apercu("Photo du chantier", f("photo.jpg"))),
        ex("plans vitrine", "Le plan, en SVG.", apercu("Plan du sous-sol", f("plan.svg"))),
        ex("plans", "La soumission.", apercu("Soumission 2026-114", f("soumission.pdf"))),
        ex("pages", "La page du projet.", apercu("Page du projet", f("page.html"))),
        ex("pages", "Une image WebP.", apercu("", f("image.webp"))),
        ex("pages", "Un GIF animé : sa première image.", apercu("Animation", f("anime.gif"))),
        ex("refus", "Trop gros pour un aperçu.", apercu("Le relevé numérisé", f("gros.png"))),
        ex("refus", "Un fichier qui n'est pas là.", apercu("Plan disparu", f("absent.png"))),
        ex("refus", "Hors du dossier de travail.", apercu("Fond d'écran", fond_d_ecran)),
        ex("refus", "Sur le réseau.", apercu("Plan du serveur", r"\\serveur\partage\plan.png".into())),
        ex("chemins", "En remontant d'un dossier.", apercu("Dehors", f(r"..\dehors.png"))),
        ex("chemins", "Par une jonction vers dehors.", apercu("Par le lien", f(r"lien\img0.jpg"))),
        ex("chemins", "Un document Word.", apercu("Rapport", f("rapport.docx"))),
        ex("chemins", "Un chemin relatif.", apercu("Plan", "plan.svg".into())),
        dessin("dessins vitrine", "Voici le plan du rez-de-chaussée.", DESSIN_PLAN, false),
        dessin("dessins", "Tes dépenses d'octobre, par catégorie.", DESSIN_GRAPHIQUE, false),
        dessin("dessins", "Un dessin plus haut que large, écrit comme dans une page web.", DESSIN_HAUT, false),
        dessin("dessins divers", "Un dessin qui ne se lit pas : son SVG tel quel.", r#"<svg><rect width="10""#, false),
        dessin("dessins divers", "Eve le dessine encore.", r#"<svg xmlns="http://www.w3.org/2000/svg""#, true),
        ex("divers", "Une carte illisible : son JSON tel quel.", illisible),
        ex("divers", "Une sorte de carte inconnue.", inconnue),
        ExempleDeCarte {
            scene: "divers",
            avant: "Eve l'écrit encore.",
            contenu: r#"{"type":"tableau","titre":"Factures à vér"#.into(),
            dessin: false,
            en_cours: true,
            opacite: 1.0,
        },
        ExempleDeCarte {
            scene: "divers",
            avant: "Une ligne de la séance précédente.",
            contenu: factures,
            dessin: false,
            en_cours: false,
            opacite: 0.55,
        },
        ex("divers", "Un agenda vide, un tableau vide.", json!({ "type": "agenda", "titre": "Samedi" }).to_string()),
        ex("divers", "", json!({ "type": "tableau", "titre": "Rien à montrer" }).to_string()),
    ]
}

/// Les dessins de l'essai, comme Claude les écrit d'après la consigne (« DESSINS ») : un plan, avec ses pièces, leurs
/// aplats et leurs cotes ; un graphique à barres, ses axes et ses valeurs écrites.
const DESSIN_PLAN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 500" font-family="Segoe UI">
<title>Plan du rez-de-chaussée</title>
<rect x="43" y="43" width="354" height="254" fill="#2563EB" fill-opacity="0.15"/>
<rect x="403" y="43" width="354" height="254" fill="#059669" fill-opacity="0.15"/>
<rect x="43" y="303" width="714" height="154" fill="#D97706" fill-opacity="0.15"/>
<rect x="40" y="40" width="720" height="420" fill="none" stroke="#1F2937" stroke-width="6"/>
<line x1="400" y1="40" x2="400" y2="300" stroke="#1F2937" stroke-width="4"/>
<line x1="40" y1="300" x2="760" y2="300" stroke="#1F2937" stroke-width="4"/>
<path d="M 330 300 A 50 50 0 0 1 380 250" fill="none" stroke="#DC2626" stroke-width="2" stroke-dasharray="6 4"/>
<text x="220" y="170" font-size="22" text-anchor="middle" fill="#1F2937">Salon</text>
<text x="220" y="198" font-size="15" text-anchor="middle" fill="#6B7280">4,2 × 3,8 m</text>
<text x="580" y="170" font-size="22" text-anchor="middle" fill="#1F2937">Cuisine</text>
<text x="580" y="198" font-size="15" text-anchor="middle" fill="#6B7280">4,2 × 3,8 m</text>
<text x="400" y="385" font-size="22" text-anchor="middle" fill="#1F2937">Chambres et salle de bain</text>
</svg>"##;

const DESSIN_GRAPHIQUE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 800 450" font-family="Segoe UI">
<title>Dépenses d'octobre, par catégorie</title>
<line x1="90" y1="380" x2="760" y2="380" stroke="#1F2937" stroke-width="2"/>
<line x1="90" y1="50" x2="90" y2="380" stroke="#1F2937" stroke-width="2"/>
<g font-size="14" fill="#6B7280" text-anchor="end">
<text x="80" y="385">0 $</text><text x="80" y="207">500 $</text><text x="80" y="57">900 $</text>
</g>
<rect x="130" y="89" width="110" height="291" fill="#2563EB"/>
<rect x="290" y="270" width="110" height="110" fill="#059669"/>
<rect x="450" y="167" width="110" height="213" fill="#D97706"/>
<rect x="610" y="327" width="110" height="53" fill="#7C3AED"/>
<g font-size="16" fill="#1F2937" text-anchor="middle">
<text x="185" y="80">820 $</text><text x="345" y="261">310 $</text><text x="505" y="158">600 $</text><text x="665" y="318">150 $</text>
<text x="185" y="405">Épicerie</text><text x="345" y="405">Essence</text><text x="505" y="405">Garderie</text><text x="665" y="405">Loisirs</text>
</g>
</svg>"##;

/// Un dessin écrit comme dans une page web (sans espace de noms), plus haut que large : il prend la hauteur de la
/// conversation, pas toute sa largeur.
const DESSIN_HAUT: &str = r##"<svg viewBox="0 0 400 700">
<title>L'arbre de la cour</title>
<rect x="180" y="420" width="40" height="230" fill="#92400E"/>
<circle cx="200" cy="300" r="170" fill="#059669" fill-opacity="0.3" stroke="#059669" stroke-width="4"/>
<circle cx="140" cy="260" r="16" fill="#DC2626"/><circle cx="250" cy="330" r="16" fill="#DC2626"/>
<line x1="20" y1="650" x2="380" y2="650" stroke="#1F2937" stroke-width="3"/>
<text x="200" y="690" font-size="26" text-anchor="middle">Le pommier</text>
</svg>"##;

struct EssaiDesCartes {
    cartes: crate::cartes::Cartes,
    exemples: Vec<ExempleDeCarte>,
    geste: String,
}

impl eframe::App for EssaiDesCartes {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        use eframe::egui::{self, Align, Layout, Margin, RichText, vec2};

        use crate::natif::couleurs;
        egui::Panel::top("essai-barre")
            .exact_size(44.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(couleurs::BARRE_HAUT).inner_margin(Margin::symmetric(16, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(crate::natif::semi("Essai des cartes", 15.0).color(egui::Color32::WHITE));
                    let aide = "à gauche, la conversation à côté du centre de commande ; à droite, le centre replié";
                    ui.label(RichText::new(aide).size(12.0).color(couleurs::BARRE_TEXTE));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let geste = if self.geste.is_empty() { "Aucun geste" } else { self.geste.as_str() };
                        ui.add(
                            egui::Label::new(RichText::new(geste).size(12.0).color(couleurs::BARRE_CLAIR)).truncate(),
                        );
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(couleurs::SURFACE).inner_margin(Margin::symmetric(16, 12)))
            .show(ui, |ui| {
                let (h, etroite) = (ui.available_height(), 418.0);
                let large = (ui.available_width() - etroite - 28.0).max(300.0);
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 28.0;
                    for (k, largeur) in [(0, etroite), (1, large)] {
                        ui.allocate_ui_with_layout(vec2(largeur, h), Layout::top_down(Align::Min), |ui| {
                            ui.set_width(largeur);
                            egui::ScrollArea::vertical().id_salt(("colonne", k)).auto_shrink([false, false]).show(
                                ui,
                                |ui| {
                                    for e in &self.exemples {
                                        if let Some(g) = bulle_d_essai(ui, &mut self.cartes, e) {
                                            let valeur = g.valeur.as_str().map_or(g.valeur.to_string(), str::to_string);
                                            self.geste = format!("Geste : {} — {valeur}", g.action);
                                        }
                                    }
                                },
                            );
                        });
                    }
                });
            });
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// Une bulle d'Eve comme celles du fil (la petite orbe, le cadre aux coins inégaux), pleine largeur parce qu'elle
/// porte une carte.
fn bulle_d_essai(
    ui: &mut eframe::egui::Ui,
    cartes: &mut crate::cartes::Cartes,
    e: &ExempleDeCarte,
) -> Option<crate::cartes::Geste> {
    use eframe::egui::{self, CornerRadius, Margin, RichText};

    use crate::natif::couleurs;
    let mut geste = None;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        crate::natif::avatar(ui, false, e.opacite);
        ui.vertical(|ui| {
            egui::Frame::new()
                .fill(couleurs::SURFACE_2.gamma_multiply(e.opacite))
                .stroke((1.0, couleurs::FILET.gamma_multiply(e.opacite)))
                .corner_radius(CornerRadius { nw: 4, ne: 14, sw: 14, se: 14 })
                .inner_margin(Margin { left: 12, right: 12, top: 8, bottom: 9 })
                .show(ui, |ui| {
                    // Toute la largeur, marges et trait du cadre ôtés (egui compte le trait dans la taille du cadre).
                    ui.set_width(ui.available_width());
                    if !e.avant.is_empty() {
                        let encre = couleurs::ENCRE.gamma_multiply(e.opacite);
                        ui.label(RichText::new(e.avant).size(14.0).color(encre));
                    }
                    geste = if e.dessin {
                        cartes.dessin(ui, &e.contenu, !e.en_cours, e.opacite)
                    } else {
                        cartes.dessiner(ui, &e.contenu, e.en_cours, e.opacite)
                    };
                });
        });
    });
    ui.add_space(12.0);
    geste
}

// ---- La musique du centre de commande (le lot 6) ------------------------------------------------------------------

/// Un geste du scénario de `eve --essai-musique` : ce que la fenêtre native dira à la musique, à son heure. `Phase`
/// ne fait que nommer ce qui suit, pour le bilan.
#[derive(Clone, Copy, Debug)]
enum GesteMusical {
    Phase(&'static str),
    Etat(&'static str),
    Sujet(&'static str),
    Nuit(bool),
    Aller,
    Retour(bool),
    Arrivee,
}

/// Les gestes d'outils qui partent toutes les 1,6 s de `debut` à `fin` : le départ, l'arrivée de l'impulsion, le
/// retour (une fois sur quatre en erreur), l'arrivée du retour — le rythme de Claude qui enchaîne les lectures.
fn outils_musicaux(debut: f64, fin: f64) -> Vec<(f64, GesteMusical)> {
    let mut v = Vec::new();
    let (mut t, mut k) = (debut, 0);
    while t + 2.0 < fin {
        v.extend([
            (t, GesteMusical::Aller),
            (t + 0.7, GesteMusical::Arrivee),
            (t + 1.2, GesteMusical::Retour(k % 4 == 3)),
            (t + 1.9, GesteMusical::Arrivee),
        ]);
        (t, k) = (t + 1.6, k + 1);
    }
    v
}

/// Le scénario sur `secondes` : Eve au repos (le premier quart) ; au travail (des outils, et le sujet « ecole » au
/// milieu) ; elle parle (la musique se tait) ; de nouveau au repos ; puis la nuit.
fn scenario_musical(secondes: f64) -> Vec<(f64, GesteMusical)> {
    use GesteMusical::{Etat, Nuit, Phase, Sujet};
    let s = secondes;
    let (travail, parole) = (0.25 * s, 0.6 * s);
    let mut v =
        vec![(0.0, Phase("repos")), (0.0, Etat("pret")), (travail, Phase("travail")), (travail, Etat("reflexion"))];
    v.extend(outils_musicaux(travail + 0.3, parole));
    v.extend([
        (0.42 * s, Sujet("ecole")),
        (0.54 * s, Sujet("aucun")),
        (parole, Phase("parole")),
        (parole, Etat("parole")),
        (0.67 * s, Phase("reprise")),
        (0.67 * s, Etat("pret")),
        (0.85 * s, Phase("nuit")),
        (0.85 * s, Nuit(true)),
    ]);
    // Un tri stable : la phase reste devant l'état qu'elle nomme.
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

/// Les phases d'un rendu : leur nom, leur début et leur fin (en échantillons).
type PhasesMusicales = Vec<(&'static str, usize, usize)>;

/// Le scénario rendu hors ligne par le synthétiseur de la musique — le code même du rappel du son —, par tampons de
/// 10 ms comme WASAPI : les deux canaux, les phases, le temps de calcul.
fn rendre_le_scenario(
    taux: u32,
    gestes: &[(f64, GesteMusical)],
    secondes: f64,
) -> (Vec<f32>, Vec<f32>, PhasesMusicales, Duration) {
    use crate::musique::{Consigne, Evenement, Synthe, gamme_du_sujet, sorte_d_etat};
    let n = (secondes * f64::from(taux)) as usize;
    let mut synthe = Synthe::nouveau(taux, 20_261_023);
    let mut c = Consigne::default();
    synthe.consigne(&c);
    let (mut g, mut d) = (vec![0.0f32; n], vec![0.0f32; n]);
    let mut phases: PhasesMusicales = Vec::new();
    let (mut suivant, mut i) = (0, 0);
    let tampon = (taux / 100).max(1) as usize;
    let debut = Instant::now();
    while i < n {
        while suivant < gestes.len() && gestes[suivant].0 * f64::from(taux) <= i as f64 {
            match gestes[suivant].1 {
                GesteMusical::Phase(nom) => {
                    if let Some(p) = phases.last_mut() {
                        p.2 = i;
                    }
                    phases.push((nom, i, n));
                }
                GesteMusical::Etat(e) => (c.travail, c.silence) = sorte_d_etat(e),
                GesteMusical::Sujet(s) => c.gamme = gamme_du_sujet(s),
                GesteMusical::Nuit(oui) => c.nuit = oui,
                GesteMusical::Aller => synthe.evenement(Evenement::Aller),
                GesteMusical::Retour(e) => synthe.evenement(if e { Evenement::Erreur } else { Evenement::Retour }),
                GesteMusical::Arrivee => synthe.evenement(Evenement::Arrivee),
            }
            synthe.consigne(&c);
            suivant += 1;
        }
        let j = (i + tampon).min(n);
        synthe.rendre(&mut g[i..j], &mut d[i..j]);
        i = j;
    }
    (g, d, phases, debut.elapsed())
}

/// La puissance moyenne d'un son (le carré de sa moyenne quadratique).
fn puissance(x: &[f32]) -> f64 {
    x.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / x.len().max(1) as f64
}

/// Une puissance en décibels.
fn decibels(p: f64) -> f64 {
    10.0 * p.max(1e-24).log10()
}

/// Le centile `q` (de 0 à 1) d'une série.
fn centile(v: &[f32], q: f64) -> f32 {
    let mut w = v.to_vec();
    w.sort_by(f32::total_cmp);
    w.get(((w.len().max(1) - 1) as f64 * q).round() as usize).copied().unwrap_or(0.0)
}

/// La puissance moyenne d'un son par fréquence (Welch : des fenêtres de Hann de `n` points, à moitié superposées).
fn spectre(x: &[f32], n: usize) -> Vec<f64> {
    let fft = crate::musique::Fft::nouvelle(n);
    let hann: Vec<f32> =
        (0..n).map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos()) as f32).collect();
    let (mut re, mut im) = (vec![0.0f32; n], vec![0.0f32; n]);
    let mut p = vec![0.0f64; n / 2];
    let (mut fenetres, mut debut) = (0u32, 0);
    while debut + n <= x.len() {
        for i in 0..n {
            re[i] = x[debut + i] * hann[i];
            im[i] = 0.0;
        }
        fft.transformer(&mut re, &mut im, false);
        for (k, v) in p.iter_mut().enumerate() {
            *v += f64::from(re[k] * re[k] + im[k] * im[k]);
        }
        fenetres += 1;
        debut += n / 2;
    }
    p.iter_mut().for_each(|v| *v /= f64::from(fenetres.max(1)));
    p
}

/// La part de chaque octave (de 63 Hz à 16 kHz) dans la puissance d'un son.
fn octaves(x: &[f32], taux: u32) -> Vec<(f64, f64)> {
    let n = 8192;
    let p = spectre(x, n);
    let total: f64 = p.iter().sum::<f64>().max(1e-30);
    let largeur = f64::from(taux) / n as f64;
    [63.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0]
        .into_iter()
        .map(|fc: f64| {
            let (bas, haut) = (fc / 2f64.sqrt(), fc * 2f64.sqrt());
            let dans = |k: usize| (bas..haut).contains(&(k as f64 * largeur));
            (fc, p.iter().enumerate().filter(|&(k, _)| dans(k)).map(|(_, v)| v).sum::<f64>() / total)
        })
        .collect()
}

/// Le temps (en s, par fenêtres de 10 ms) qu'un son met, depuis `depuis`, à passer sous `seuil` décibels de la
/// puissance `reference` — ou au-dessus, si `monter`.
fn temps_pour(x: &[f32], depuis: usize, taux: u32, reference: f64, seuil: f64, monter: bool) -> Option<f64> {
    let f = (taux / 100).max(1) as usize;
    x[depuis.min(x.len())..]
        .chunks(f)
        .position(|b| (decibels(puissance(b) / reference) > seuil) == monter)
        .map(|k| k as f64 * 0.01)
}

/// Un WAV PCM 16 bits, les canaux entrelacés.
fn ecrire_wav(chemin: &Path, canaux: &[&[f32]], taux: u32) -> std::io::Result<()> {
    let n = canaux.iter().map(|c| c.len()).min().unwrap_or(0);
    let k = canaux.len() as u16;
    let donnees = (n * canaux.len() * 2) as u32;
    let mut o = Vec::with_capacity(44 + donnees as usize);
    o.extend_from_slice(b"RIFF");
    o.extend_from_slice(&(36 + donnees).to_le_bytes());
    o.extend_from_slice(b"WAVEfmt ");
    o.extend_from_slice(&16u32.to_le_bytes());
    o.extend_from_slice(&1u16.to_le_bytes());
    o.extend_from_slice(&k.to_le_bytes());
    o.extend_from_slice(&taux.to_le_bytes());
    o.extend_from_slice(&(taux * u32::from(k) * 2).to_le_bytes());
    o.extend_from_slice(&(k * 2).to_le_bytes());
    o.extend_from_slice(&16u16.to_le_bytes());
    o.extend_from_slice(b"data");
    o.extend_from_slice(&donnees.to_le_bytes());
    for i in 0..n {
        for c in canaux {
            o.extend_from_slice(&((c[i].clamp(-1.0, 1.0) * 32_767.0).round() as i16).to_le_bytes());
        }
    }
    std::fs::write(chemin, o)
}

/// Le micro par défaut tel qu'il entend (mis en mono, à sa cadence), enregistré à côté de celui d'Eve : le flux, les
/// échantillons, la cadence, le nom.
type Enregistrement = (cpal::Stream, Arc<Mutex<Vec<f32>>>, u32, String);

fn enregistrer_le_micro(secondes: u64) -> Result<Enregistrement, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let appareil = cpal::default_host().default_input_device().ok_or("aucun micro branché")?;
    let nom = appareil.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "micro".into());
    let cfg = appareil.default_input_config().map_err(|e| format!("le micro ne dit pas son format : {e}"))?;
    let (taux, canaux) = (cfg.sample_rate(), usize::from(cfg.channels()).max(1));
    let tampon = Arc::new(Mutex::new(Vec::with_capacity(taux as usize * secondes as usize)));
    let t = tampon.clone();
    let flux = appareil
        .build_input_stream_raw(
            cfg.config(),
            cfg.sample_format(),
            move |donnees: &cpal::Data, _: &cpal::InputCallbackInfo| {
                let Ok(mut v) = t.lock() else { return };
                if let Some(x) = donnees.as_slice::<f32>() {
                    v.extend(x.chunks(canaux).map(|tr| tr.iter().sum::<f32>() / tr.len() as f32));
                } else if let Some(x) = donnees.as_slice::<i16>() {
                    let m = |tr: &[i16]| tr.iter().map(|&s| f32::from(s) / 32_768.0).sum::<f32>() / tr.len() as f32;
                    v.extend(x.chunks(canaux).map(m));
                }
            },
            |e: cpal::Error| {
                if !matches!(e.kind(), cpal::ErrorKind::Xrun) {
                    eprintln!("micro : {e}");
                }
            },
            None,
        )
        .map_err(|e| format!("le micro ne s'ouvre pas : {e}"))?;
    flux.play().map_err(|e| format!("le micro ne démarre pas : {e}"))?;
    Ok((flux, tampon, taux, nom))
}

/// Ce que le détecteur de parole d'Eve a fait pendant une phase : les phrases qu'il a commencées, celles qu'il a
/// finies (qu'Eve donnerait à l'oreille), le temps qu'il a cru entendre parler.
#[derive(Clone, Copy, Default)]
struct Detections {
    debuts: u32,
    phrases: u32,
    parole: f64,
}

/// Une phase de l'essai en vrai : son nom, ses échantillons du micro, ce qu'a fait le détecteur, et le relevé de la
/// musique à son début et à sa fin.
struct PhaseEntendue {
    nom: &'static str,
    debut: usize,
    fin: usize,
    det: Detections,
    avant: crate::musique::Releve,
    apres: crate::musique::Releve,
}

/// Les pas de `--marge`, de 3 dB en 3 dB au-dessus de la musique de l'essai.
const PAS_DE_LA_MARGE: [&str; 8] = ["+3 dB", "+6 dB", "+9 dB", "+12 dB", "+15 dB", "+18 dB", "+21 dB", "+24 dB"];

/// L'écoute de l'essai : le micro d'Eve et son détecteur, l'enregistrement brut, la phase en cours, les phases finies.
struct SeanceDEcoute {
    micro: micro::Micro,
    brut: Arc<Mutex<Vec<f32>>>,
    taux: u32,
    etait: bool,
    phase: PhaseEntendue,
    entendues: Vec<PhaseEntendue>,
    /// La garde : pendant la musique, un bloc de 20 ms du micro plus fort que ce niveau la fait taire aussitôt — avant
    /// que le détecteur ne la prenne pour une voix, le nôtre comme celui d'une autre Eve qui écouterait le même micro
    /// (son oreille pourrait y entendre « Eve », et donner la suite à Claude).
    garde: Option<f32>,
    /// Le niveau qui a fait jouer la garde, et la phase où il est venu.
    alerte: Option<(f32, &'static str)>,
}

impl SeanceDEcoute {
    fn position(&self) -> usize {
        self.brut.lock().map(|v| v.len()).unwrap_or(0)
    }

    /// Les 20 derniers millisecondes du micro (le bloc du détecteur), en moyenne quadratique.
    fn dernier_bloc(&self) -> f32 {
        let n = (self.taux / 50).max(1) as usize;
        self.brut.lock().map(|v| micro::rms(&v[v.len().saturating_sub(n)..])).unwrap_or(0.0)
    }

    /// Écoute le détecteur de parole d'Eve jusqu'à `fin`, toutes les 10 ms ; s'arrête tôt si la garde, levée, a joué.
    fn ecouter(&mut self, fin: Instant) {
        while Instant::now() < fin && !(self.garde.is_some() && self.alerte.is_some()) {
            let parle = self.micro.en_parole();
            if parle && !self.etait {
                self.phase.det.debuts += 1;
            }
            if parle {
                self.phase.det.parole += 0.01;
            }
            self.etait = parle;
            while self.micro.prendre_phrase().is_some() {
                self.phase.det.phrases += 1;
            }
            if let Some(seuil) = self.garde {
                let niveau = self.dernier_bloc();
                if niveau > seuil {
                    self.alerte = Some((niveau, self.phase.nom));
                    return;
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Ferme la phase en cours (si elle a un nom) et en ouvre une autre (un nom vide : aucune).
    fn changer(&mut self, nom: &'static str, releve: crate::musique::Releve) {
        let n = self.position();
        let suivante =
            PhaseEntendue { nom, debut: n, fin: n, det: Detections::default(), avant: releve, apres: releve };
        let finie = std::mem::replace(&mut self.phase, suivante);
        if !finie.nom.is_empty() {
            self.entendues.push(PhaseEntendue { fin: n, apres: releve, ..finie });
        }
    }

    /// Joue des gestes à leur heure (comptée depuis `depart`), en écoutant le détecteur entre deux. La garde qui joue
    /// fait taire la musique (le fondu de 50 ms du silence d'Eve) et arrête le scénario.
    fn jouer(&mut self, musique: &mut crate::musique::Musique, gestes: &[(f64, GesteMusical)], depart: Instant) {
        for &(t, geste) in gestes {
            self.ecouter(depart + Duration::from_secs_f64(t));
            if self.alerte.is_some() {
                musique.etat("parole");
                return;
            }
            match geste {
                GesteMusical::Phase(nom) => self.changer(nom, musique.releve()),
                GesteMusical::Etat(e) => musique.etat(e),
                GesteMusical::Sujet(x) => musique.sujet(x),
                GesteMusical::Nuit(oui) => musique.nuit(oui),
                GesteMusical::Aller => musique.aller(),
                GesteMusical::Retour(erreur) => musique.retour(erreur),
                GesteMusical::Arrivee => musique.arrivee(),
            }
        }
    }
}

/// `eve --essai-musique [secondes] [dossier] [--gain DB] [--marge] [--sans-micro]` : la musique du centre de commande.
///
/// D'abord hors ligne (le synthétiseur seul, le code même du rappel du son) : son niveau à chaque phase du scénario
/// ([`scenario_musical`], 60 s d'office), son spectre, ses fondus, son coût. Puis en vrai, par la sortie par défaut,
/// pendant que le micro écoute avec le détecteur de parole d'Eve (`micro::Micro`, l'écoute continue du mot d'éveil) :
/// 2 s pour que le micro s'installe, 10 s de calme (le bruit du bureau seul), le scénario (plus fort ou plus bas de
/// `--gain` décibels), 5 s de calme encore. Pour chaque phase : ce que le micro entend (en blocs de 20 ms, ceux du
/// détecteur), la part de la musique, le niveau de sortie, la marge jusqu'au seuil du détecteur, et les phrases qu'il
/// aurait données à l'oreille. `--marge` : ensuite, la musique plus forte de 3 dB en 3 dB (12 s chaque fois, Eve au
/// travail), jusqu'à la garde. Un dossier : `musique.wav` (le rendu) et `micro.wav` (ce que le micro a entendu).
///
/// La garde (la moitié du seuil le plus bas du détecteur) fait taire la musique avant que le micro ne l'entende assez
/// fort pour une voix : une autre Eve qui écouterait le mot d'éveil sur ce poste ne doit rien en recevoir.
pub fn musique(args: &[String]) -> i32 {
    use cpal::traits::{DeviceTrait, HostTrait};

    use crate::musique::{self as m, Musique, Releve};
    let (mut secondes, mut dossier, mut gain_db, mut marge, mut sans_micro) = (60u64, None, 0.0f64, false, false);
    let mut mots = args.iter();
    while let Some(a) = mots.next() {
        match a.as_str() {
            "--marge" => marge = true,
            "--sans-micro" => sans_micro = true,
            "--gain" => gain_db = mots.next().and_then(|v| v.parse().ok()).unwrap_or(0.0),
            _ => match a.parse::<u64>() {
                Ok(n) => secondes = n.clamp(20, 600),
                Err(_) => dossier = Some(std::path::PathBuf::from(a)),
            },
        }
    }
    if let Some(d) = &dossier
        && let Err(e) = std::fs::create_dir_all(d)
    {
        eprintln!("{} : {e}", d.display());
        return 1;
    }
    let taux = cpal::default_host()
        .default_output_device()
        .and_then(|a| a.default_output_config().ok())
        .map_or(48_000, |c| c.sample_rate());
    let s = secondes as f64;
    let gestes = scenario_musical(s);

    // ---- Hors ligne ----
    let (g, d, phases, calcul) = rendre_le_scenario(taux, &gestes, s);
    println!(
        "musique : VOLUME {:.3} ({:+.1} dB par rapport à la page) ; {taux} Hz ; un scénario de {secondes} s",
        m::VOLUME,
        20.0 * m::VOLUME.log10()
    );
    println!(
        "rendu hors ligne : {:.2} s de calcul pour {secondes} s de musique, {:.2} % d'un cœur",
        calcul.as_secs_f64(),
        calcul.as_secs_f64() / s * 100.0
    );
    println!("phase        niveau (RMS)   crête        le plus fort bloc de 20 ms");
    let bloc = (taux / 50) as usize;
    for &(nom, a, b) in &phases {
        let p = (puissance(&g[a..b]) + puissance(&d[a..b])) / 2.0;
        let crete = g[a..b].iter().chain(&d[a..b]).fold(0.0f32, |c, v| c.max(v.abs()));
        let fort = g[a..b]
            .chunks_exact(bloc)
            .zip(d[a..b].chunks_exact(bloc))
            .map(|(x, y)| (puissance(x) + puissance(y)) / 2.0)
            .fold(0.0, f64::max);
        let crete = 20.0 * f64::from(crete).max(1e-12).log10();
        println!("{nom:<12} {:>7.1} dBFS   {crete:>6.1} dBFS   {:>6.1} dBFS", decibels(p), decibels(fort));
    }
    let mono: Vec<f32> = phases
        .iter()
        .filter(|p| p.0 == "repos" || p.0 == "travail")
        .flat_map(|&(_, a, b)| (a..b).map(|i| (g[i] + d[i]) / 2.0))
        .collect();
    let bandes: Vec<String> =
        octaves(&mono, taux).iter().map(|(fc, part)| format!("{fc:.0} Hz {:.1} dB", decibels(*part))).collect();
    println!("spectre (repos et travail), la part de chaque octave : {}", bandes.join(" · "));
    if let (Some(p), Some(r)) = (phases.iter().find(|p| p.0 == "parole"), phases.iter().find(|p| p.0 == "reprise")) {
        let reference = puissance(&g[p.1.saturating_sub(taux as usize)..p.1]);
        let sous = |seuil| temps_pour(&g, p.1, taux, reference, seuil, false);
        let regime = puissance(&g[r.2.saturating_sub(3 * taux as usize)..r.2]);
        let retour = temps_pour(&g, r.1, taux, regime, -6.0, true);
        let ecrit = |t: Option<f64>| t.map_or("jamais".to_string(), |t| format!("{t:.2} s"));
        println!(
            "Eve parle : la musique perd 40 dB en {}, 60 dB en {} ; elle revient (à −6 dB) {} après",
            ecrit(sous(-40.0)),
            ecrit(sous(-60.0)),
            ecrit(retour)
        );
    }
    if let Some(dir) = &dossier {
        match ecrire_wav(&dir.join("musique.wav"), &[&g, &d], taux) {
            Ok(()) => println!("écrit : {}", dir.join("musique.wav").display()),
            Err(e) => eprintln!("musique.wav : {e}"),
        }
    }
    if sans_micro {
        return 0;
    }

    // ---- En vrai : la sortie par défaut, le micro et le détecteur de parole d'Eve ----
    let micro_d_eve = match micro::Micro::ouvrir() {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    micro_d_eve.ecoute_continue(true);
    let (flux, brut, taux_micro, nom_micro) = match enregistrer_le_micro(secondes + 120) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    println!("\nmicro : {nom_micro}, {taux_micro} Hz ; le détecteur de parole d'Eve écoute (l'écoute du mot d'éveil)");
    let vide = PhaseEntendue {
        nom: "",
        debut: 0,
        fin: 0,
        det: Detections::default(),
        avant: Releve::default(),
        apres: Releve::default(),
    };
    let mut ecoute = SeanceDEcoute {
        micro: micro_d_eve,
        brut,
        taux: taux_micro,
        etait: false,
        phase: vide,
        entendues: Vec::new(),
        garde: None,
        alerte: None,
    };
    // Le micro s'installe (le détecteur apprend le bruit de fond) : 2 s qui ne comptent pas.
    ecoute.ecouter(Instant::now() + Duration::from_secs(2));
    ecoute.changer("calme", Releve::default());
    ecoute.ecouter(Instant::now() + Duration::from_secs(10));
    ecoute.changer("", Releve::default());
    let Some(mut musique) = Musique::nouvelle() else {
        eprintln!("aucune sortie audio : rien à mesurer");
        return 1;
    };
    let taux_sortie = musique.taux;
    let gain = 10f64.powf(gain_db / 20.0);
    println!("sortie : {}, {taux_sortie} Hz ; la musique à {gain_db:+.1} dB de VOLUME", musique.nom);
    musique.regler(true, true);
    musique.amplifier(gain as f32);
    ecoute.garde = Some(micro::SEUIL_MIN / 2.0);
    let depart = Instant::now();
    ecoute.jouer(&mut musique, &gestes, depart);
    ecoute.ecouter(depart + Duration::from_secs_f64(s));
    let releve = musique.releve();
    ecoute.garde = None;
    ecoute.changer("après", releve);
    drop(musique);
    ecoute.ecouter(Instant::now() + Duration::from_secs(5));
    ecoute.changer("", releve);
    // La marge : la musique de 3 dB en 3 dB plus forte, Eve au travail, jusqu'à la garde — ou jusqu'au niveau de la
    // voix d'Eve (−14 dBFS) : au-delà, la question ne se pose plus.
    let mut sans_garde: Option<f64> = None;
    if marge
        && ecoute.alerte.is_none()
        && let Some(mut forte) = Musique::nouvelle()
    {
        forte.regler(true, true);
        forte.etat("reflexion");
        ecoute.garde = Some(micro::SEUIL_MIN / 2.0);
        for k in 1..=8 {
            let (nom, db) = (PAS_DE_LA_MARGE[k - 1], 3.0 * k as f64);
            forte.amplifier((gain * 10f64.powf(db / 20.0)) as f32);
            ecoute.changer(nom, forte.releve());
            let debut = Instant::now();
            ecoute.jouer(&mut forte, &outils_musicaux(0.3, 12.0), debut);
            ecoute.ecouter(debut + Duration::from_secs(12));
            if ecoute.alerte.is_some() {
                break;
            }
            sans_garde = Some(db);
            let (avant, apres) = (ecoute.phase.avant, forte.releve());
            let trames = apres.trames.saturating_sub(avant.trames).max(1);
            if decibels((apres.energie - avant.energie) / trames as f64) > -14.0 {
                break;
            }
        }
        ecoute.garde = None;
        ecoute.changer("", forte.releve());
    }
    let alerte = ecoute.alerte;
    drop(flux);
    let brut = ecoute.brut.lock().map(|v| v.clone()).unwrap_or_default();
    let entendues = ecoute.entendues;

    // ---- Le bilan ----
    // Le détecteur part sur 60 ms (trois blocs de 20 ms) au-dessus de max(3 × le bruit de fond, SEUIL_MIN) : le bruit
    // de fond, c'est à peu près la moyenne des blocs du calme.
    let bloc = (taux_micro / 50).max(1) as usize;
    let tranche = |p: &PhaseEntendue| &brut[p.debut.min(brut.len())..p.fin.min(brut.len())];
    let blocs_de = |p: &PhaseEntendue| tranche(p).chunks_exact(bloc).map(micro::rms).collect::<Vec<f32>>();
    let calme = entendues.iter().find(|p| p.nom == "calme");
    let p_calme = calme.map_or(0.0, |c| puissance(tranche(c)));
    let bruit = calme.map(blocs_de).filter(|b| !b.is_empty()).map_or(0.0, |b| b.iter().sum::<f32>() / b.len() as f32);
    let seuil = (3.0 * bruit).max(micro::SEUIL_MIN);
    println!(
        "\nle seuil du détecteur : 60 ms au-dessus de {seuil:.4} ({:.1} dBFS) — trois fois le bruit du calme \
         ({bruit:.4}), {} dBFS au moins",
        20.0 * seuil.log10(),
        (20.0 * micro::SEUIL_MIN.log10()).round()
    );
    println!(
        "phase        au micro (20 ms) : médiane  p95     max     60 ms tenus  marge     musique au micro  \
         sortie       couplage   détecteur : commence finit parle"
    );
    for p in &entendues {
        let x = tranche(p);
        let blocs = blocs_de(p);
        // Le niveau le plus fort que le micro a tenu 60 ms : ce que le détecteur compare à son seuil.
        let tenu = blocs.windows(3).map(|w| w[0].min(w[1]).min(w[2])).fold(0.0f32, f32::max);
        let marge_db = 20.0 * (seuil / tenu.max(1e-9)).log10();
        // La musique au micro : ce que la phase a de plus que le calme. Sous la moitié du bruit du calme, ce n'est que
        // la pièce qui varie (la phase « après », sans musique, en a autant) : pas de couplage à en tirer.
        let au_micro = (puissance(x) - p_calme).max(0.0);
        let audible = au_micro > 0.5 * p_calme;
        let trames = p.apres.trames.saturating_sub(p.avant.trames);
        let sortie = if trames > 0 { (p.apres.energie - p.avant.energie) / trames as f64 } else { 0.0 };
        let couplage = if sortie > 0.0 && audible {
            format!("{:+6.1} dB", decibels(au_micro / sortie))
        } else {
            "    —    ".to_string()
        };
        let au_micro = if audible { format!("{:>7.1} dBFS", decibels(au_micro)) } else { "sous le bruit".to_string() };
        let sortie = if sortie > 0.0 { format!("{:6.1} dBFS", decibels(sortie)) } else { "    —      ".to_string() };
        println!(
            "{:<12}                  {:.4}   {:.4}  {:.4}  {tenu:.4}       {marge_db:+5.1} dB  {au_micro:>13}     \
             {sortie}  {couplage}  {:>8}  {:>5}  {:>4.1} s",
            p.nom,
            centile(&blocs, 0.5),
            centile(&blocs, 0.95),
            centile(&blocs, 1.0),
            p.det.debuts,
            p.det.phrases,
            p.det.parole
        );
    }
    if let Some((niveau, phase)) = alerte {
        println!(
            "→ la garde a joué (phase « {phase} ») : le micro a entendu un bloc de 20 ms à {niveau:.4} ({:.1} dBFS), \
             la musique s'est tue",
            20.0 * niveau.log10()
        );
    }
    match (marge, sans_garde) {
        (true, Some(db)) => println!(
            "→ la marge : la musique monte de {db:+.0} dB au-dessus de l'essai sans que le micro l'entende à moins de \
             6 dB du seuil{}",
            if alerte.is_some() { " ; au pas suivant, la garde a joué" } else { "" }
        ),
        (true, None) => println!("→ la marge : moins de 3 dB — la garde a joué au premier pas"),
        _ => {}
    }
    // La nappe au micro, à ses propres fréquences : le repos contre le calme (le bruit du bureau seul).
    let trouver = |nom: &str| entendues.iter().find(|p| p.nom == nom);
    if let (Some(c), Some(r)) = (trouver("calme"), trouver("repos")) {
        let n = 8192;
        let (sc, sr) = (spectre(tranche(c), n), spectre(tranche(r), n));
        let largeur = f64::from(taux_micro) / n as f64;
        let raies: Vec<String> = m::nappe_de(0)
            .iter()
            .map(|&f| {
                let k = (f / largeur).round() as usize;
                let somme = |s: &[f64]| s[k.saturating_sub(1)..=(k + 1).min(s.len() - 1)].iter().sum::<f64>();
                format!("{f:.0} Hz {:+.1} dB", decibels(somme(&sr) / somme(&sc).max(1e-30)))
            })
            .collect();
        println!("la nappe au micro, à ses fréquences (le repos contre le calme) : {}", raies.join(" · "));
    }
    let compter = |noms: &[&str]| {
        entendues
            .iter()
            .filter(|p| noms.contains(&p.nom))
            .fold((0, 0), |a, p| (a.0 + p.det.debuts, a.1 + p.det.phrases))
    };
    let ((md, mp), (cd, cp)) = (compter(&["repos", "travail", "reprise", "nuit"]), compter(&["calme", "après"]));
    println!(
        "→ avec la musique : {md} phrase(s) commencée(s) par le détecteur, {mp} donnée(s) à l'oreille ; au calme : \
         {cd} et {cp}"
    );
    // Le coût du rappel du son, sur tout ce qu'il a joué, et la crête de ce qu'il a joué.
    let (trames, calcul, pire, accrocs, crete) =
        entendues.iter().filter(|p| p.apres.trames > p.avant.trames).fold((0u64, 0f64, 0f32, 0u64, 0f32), |a, p| {
            (
                a.0 + (p.apres.trames - p.avant.trames),
                a.1 + p.apres.calcul.saturating_sub(p.avant.calcul).as_secs_f64(),
                a.2.max(p.apres.pire_charge),
                a.3 + p.apres.accrocs.saturating_sub(p.avant.accrocs),
                a.4.max(p.apres.crete),
            )
        });
    let joue = trames as f64 / f64::from(taux_sortie.max(1));
    if joue > 0.0 {
        println!(
            "le rappel du son : {:.2} % d'un cœur en moyenne sur {joue:.0} s, {:.1} % de son tampon au pire ; \
             {accrocs} accroc(s) de la sortie ; sa crête, {:.1} dBFS",
            calcul / joue * 100.0,
            f64::from(pire) * 100.0,
            20.0 * f64::from(crete).max(1e-12).log10()
        );
    }
    if let Some(dir) = &dossier {
        match ecrire_wav(&dir.join("micro.wav"), &[&brut], taux_micro) {
            Ok(()) => println!("écrit : {}", dir.join("micro.wav").display()),
            Err(e) => eprintln!("micro.wav : {e}"),
        }
    }
    0
}

// ---- Le réseau des actions (le lot 6) -----------------------------------------------------------------------------

/// Les scènes de `eve --essai-reseau`.
const SCENES_DU_RESEAU: &str =
    "tout, demo, sujets, sujet NOM, teinte, personnes, voix, econome, journee EVE.LOG [AAAA-MM-JJ], mesure";

/// Une scène de l'essai du réseau.
#[derive(Clone, PartialEq)]
enum SceneDuReseau {
    /// Par cycles de 30 s : la démonstration (12 s) ; une image de `sujet.js`, avec sa personne (et sa couleur un
    /// cycle sur trois) ; la voix ; un outil qui nomme une personne, et une couleur seule un cycle sur deux.
    Tout,
    Demo,
    /// Les onze images, 9 s chacune.
    Sujets,
    Sujet(String),
    Teinte,
    Personnes,
    Voix,
    Econome,
    /// La journée d'un journal d'Eve, rejouée.
    Journee,
    /// Le repos (10 s), la démonstration (15 s), une image (15 s), tout à la fois (15 s) : ce que coûte une image.
    Mesure,
}

/// `eve --essai-reseau [scène] [--galaxie] [--mesure] [--nu] [--econome] [--secondes N] [--captures DOSSIER --aux
/// T1,T2…]` : le réseau des actions plein fenêtre, habillé comme le tableau HTML de la famille (la ligne d'état en haut,
/// la courbe d'activité en bas, des personnes d'exemple). `--galaxie` le montre en Voie lactée (le thème « galaxie ») ;
/// `--mesure` relève, phase par phase, ce que coûte une image sur le processeur (le temps de `Reseau::dessiner`, et
/// l'image entière selon eframe), et l'écrit en partant ; `--nu` montre
/// le réseau seul, avec son propre cadre ; `--captures` enregistre la fenêtre en PNG aux secondes dites (`--aux
/// 5,12.5`) ; `--secondes N` la ferme d'elle-même. Touches : D la démonstration, E le mode économe, S l'image suivante,
/// C une couleur, V la voix, P une personne, J la journée (avec un journal), Échap pour fermer.
pub fn reseau(args: &[String]) -> i32 {
    let valeur = |nom: &str| args.iter().position(|a| a == nom).and_then(|i| args.get(i + 1)).cloned();
    let a_valeur = ["--secondes", "--captures", "--aux"];
    let positionnels: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(k, a)| !a.starts_with("--") && (*k == 0 || !a_valeur.contains(&args[k - 1].as_str())))
        .map(|(_, a)| a)
        .collect();
    let nom = positionnels.first().map_or("tout", |s| s.as_str());
    let scene = match nom {
        "tout" => SceneDuReseau::Tout,
        "demo" => SceneDuReseau::Demo,
        "sujets" => SceneDuReseau::Sujets,
        "sujet" => SceneDuReseau::Sujet(positionnels.get(1).map_or("ecole".to_string(), |s| s.to_string())),
        "teinte" => SceneDuReseau::Teinte,
        "personnes" => SceneDuReseau::Personnes,
        "voix" => SceneDuReseau::Voix,
        "econome" => SceneDuReseau::Econome,
        "journee" => SceneDuReseau::Journee,
        "mesure" => SceneDuReseau::Mesure,
        _ => {
            eprintln!("scène inconnue : {nom} ({SCENES_DU_RESEAU})");
            return 2;
        }
    };
    let journal = if scene == SceneDuReseau::Journee {
        let Some(chemin) = positionnels.get(1) else {
            eprintln!("journee : le journal à rejouer manque (eve.log)");
            return 2;
        };
        let texte = match std::fs::read(chemin.as_str()) {
            Ok(o) => String::from_utf8_lossy(&o).into_owned(),
            Err(e) => {
                eprintln!("{chemin} : {e}");
                return 1;
            }
        };
        let date = positionnels.get(2).map_or_else(crate::journal::date, |d| d.to_string());
        Some((texte, date))
    } else {
        None
    };
    let mesure = scene == SceneDuReseau::Mesure || args.iter().any(|a| a == "--mesure");
    let secondes =
        valeur("--secondes").and_then(|s| s.parse::<f64>().ok()).or((scene == SceneDuReseau::Mesure).then_some(55.0));
    let econome = args.iter().any(|a| a == "--econome") || scene == SceneDuReseau::Econome;
    // Le réseau nu, avec son propre cadre et sans l'habillage de l'essai : la fenêtre de la mesure d'avant.
    let nu = args.iter().any(|a| a == "--nu");
    // La Voie lactée (le thème « galaxie ») à la place du cerveau.
    let galaxie = args.iter().any(|a| a == "--galaxie");
    let captures = valeur("--captures").map(std::path::PathBuf::from);
    let mut aux: Vec<f64> =
        valeur("--aux").map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
    aux.sort_by(f64::total_cmp);
    let titre = "Eve — essai du réseau";
    let vue = egui::ViewportBuilder::default().with_title(titre).with_inner_size([1600.0, 900.0]);
    let options = eframe::NativeOptions { viewport: vue, ..Default::default() };
    let nom = nom.to_string();
    let creer: eframe::AppCreator<'_> = Box::new(move |cc| {
        crate::natif::polices(&cc.egui_ctx);
        crate::natif::apparence(&cc.egui_ctx);
        // Le dossier de la famille, pour nommer les destinations comme le tableau : rien n'y est lu ni écrit.
        let mut reseau = crate::reseau::Reseau::nouveau(Path::new(r"C:\Exemple\Maison"));
        reseau.habiller(true, nu);
        reseau.personnes(personnes_d_exemple());
        reseau.evoquer("ecole", vec!["Léa".into(), "Noah".into()]);
        reseau.econome(econome);
        if galaxie {
            reseau.allure(crate::reseau::Allure::Galaxie);
        }
        Ok(Box::new(EssaiDuReseau {
            reseau,
            scene,
            nom,
            journal,
            secondes,
            mesure,
            nu,
            captures,
            aux,
            pas: -1,
            fins: Vec::new(),
            voix: false,
            image: 0,
            n: 0,
            releves: Vec::new(),
            images: VecDeque::new(),
            cout: 0.0,
        }))
    });
    match eframe::run_native(titre, options, creer) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("la fenêtre ne s'ouvre pas : {e}");
            1
        }
    }
}

/// Des personnes d'exemple (personne de vrai) : deux parents, deux enfants ; le premier a un mot de plus, le dernier
/// aussi (la garderie, où il va).
fn personnes_d_exemple() -> Vec<crate::reseau::Personne> {
    let p = |nom: &str, couleur: [u8; 3], motifs: &[&str]| crate::reseau::Personne {
        nom: nom.to_string(),
        couleur,
        motifs: motifs.iter().map(|m| m.to_string()).collect(),
    };
    vec![
        p("Marc", [0, 229, 255], &["chantier"]),
        p("Julie", [255, 122, 184], &[]),
        p("Léa", [255, 210, 63], &[]),
        p("Noah", [95, 255, 196], &["garderie"]),
    ]
}

/// Des outils qui nomment chacune des personnes d'exemple.
const OUTILS_DES_PERSONNES: [(&str, &str); 4] = [
    ("Read", r"C:\Exemple\Maison\chantier\soumission du garage.md"),
    ("Edit", r"C:\Exemple\Maison\Julie\rendez-vous chez le dentiste.ics"),
    ("Read", r"C:\Exemple\Maison\agenda\Léa\horaire.md"),
    ("Write", r"C:\Exemple\Maison\garderie\menu de la semaine.md"),
];

struct EssaiDuReseau {
    reseau: crate::reseau::Reseau,
    scene: SceneDuReseau,
    nom: String,
    /// Le journal à rejouer et sa date.
    journal: Option<(String, String)>,
    secondes: Option<f64>,
    mesure: bool,
    /// Sans l'habillage de l'essai (le réseau garde son cadre).
    nu: bool,
    captures: Option<std::path::PathBuf>,
    aux: Vec<f64>,
    /// Le pas de la scène déjà joué (un cycle, une image) : chacun ne se joue qu'une fois.
    pas: i64,
    /// Les fins des outils que la scène a lancés.
    fins: Vec<(f64, crate::reseau::Evenement)>,
    /// La voix simulée, l'image suivante (S), le nombre d'outils lancés.
    voix: bool,
    image: usize,
    n: u32,
    /// Par phase : son nom, le temps de `dessiner`, celui de l'image entière (ms).
    releves: Vec<(String, Vec<f64>, Vec<f64>)>,
    /// Les instants des images des deux dernières secondes (la cadence), et ce qu'a coûté `dessiner` à la dernière.
    images: VecDeque<f64>,
    cout: f64,
}

impl EssaiDuReseau {
    /// Un outil de la scène : son début maintenant, sa fin 1,5 s plus tard.
    fn outil(&mut self, outil: &str, detail: &str, t: f64) {
        self.n += 1;
        let e = crate::reseau::Evenement {
            id: format!("essai{}", self.n),
            outil: outil.into(),
            detail: detail.into(),
            fin: false,
            erreur: false,
        };
        self.reseau.recevoir(&e, t);
        self.fins.push((t + 1.5, crate::reseau::Evenement { fin: true, ..e }));
    }

    /// Une valeur de `sujet.js`, écrite à l'instant même (elle joue donc).
    fn sujet_js(&mut self, mut valeur: Value, t: f64) {
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
        valeur["t"] = Value::from(unix);
        self.reseau.sujet_js(&valeur, unix, t);
    }

    /// Le pas `pas` de la scène commence-t-il ? (Chacun ne se joue qu'une fois.)
    fn nouveau(&mut self, pas: i64) -> bool {
        std::mem::replace(&mut self.pas, pas) != pas
    }

    /// La scène avance ; rend le nom de la phase en cours (pour la mesure).
    fn jouer(&mut self, t: f64) -> String {
        use crate::reseau::IMAGES;
        use serde_json::json;
        let couleurs: [[u8; 3]; 4] = [[255, 200, 40], [90, 190, 255], [255, 122, 184], [124, 255, 107]];
        let phase = match self.scene.clone() {
            SceneDuReseau::Tout => {
                let cycle = (t / 30.0).floor() as i64;
                let dans = t - cycle as f64 * 30.0;
                self.reseau.demo(dans < 12.0);
                self.voix = (16.0..20.0).contains(&dans);
                let k = cycle as usize;
                if (12.0..24.0).contains(&dans) && self.nouveau(cycle * 3 + 1) {
                    let mut v = json!({ "s": IMAGES[k % IMAGES.len()].0, "p": k % 4 });
                    if k % 3 == 2 {
                        v["c"] = json!(couleurs[k % 4]);
                    }
                    self.sujet_js(v, t);
                } else if dans >= 24.0 && self.nouveau(cycle * 3 + 2) {
                    let (outil, detail) = OUTILS_DES_PERSONNES[(k + 1) % 4];
                    self.outil(outil, detail, t);
                    if k % 2 == 1 {
                        self.sujet_js(json!({ "c": couleurs[(k + 1) % 4], "d": 5000 }), t);
                    }
                }
                "tout".to_string()
            }
            SceneDuReseau::Demo | SceneDuReseau::Econome => {
                self.reseau.demo(true);
                self.nom.clone()
            }
            SceneDuReseau::Sujets => {
                let k = (t / 9.0).floor() as i64;
                if self.nouveau(k) {
                    let k = k as usize;
                    let mut v = json!({ "s": IMAGES[k % IMAGES.len()].0, "p": k % 4, "d": 6000 });
                    if k % 3 == 2 {
                        v["c"] = json!(couleurs[k % 4]);
                    }
                    self.sujet_js(v, t);
                }
                format!("sujet {}", IMAGES[(t / 9.0).floor() as usize % IMAGES.len()].0)
            }
            SceneDuReseau::Sujet(nom) => {
                if self.nouveau(0) {
                    self.sujet_js(json!({ "s": nom, "p": 2, "d": 1.0e9 }), t);
                }
                format!("sujet {nom}")
            }
            SceneDuReseau::Teinte => {
                self.reseau.demo(true);
                let k = (t / 10.0).floor() as i64;
                if self.nouveau(k) {
                    self.sujet_js(json!({ "c": couleurs[k as usize % 4], "d": 8000 }), t);
                }
                "teinte".to_string()
            }
            SceneDuReseau::Personnes => {
                // Un outil qui nomme quelqu'un, puis une phrase entendue, chacun son tour.
                let k = (t / 3.0).floor() as i64;
                if self.nouveau(k) {
                    let k = k as usize;
                    if k % 2 == 0 {
                        let (outil, detail) = OUTILS_DES_PERSONNES[k / 2 % 4];
                        self.outil(outil, detail, t);
                    } else {
                        let phrases = ["Noah a-t-il son lunch pour la garderie ?", "Julie paie la facture d'Hydro"];
                        self.reseau.entendu(phrases[k / 2 % 2], t);
                    }
                }
                "personnes".to_string()
            }
            SceneDuReseau::Voix => {
                self.reseau.demo(true);
                self.voix = true;
                "voix".to_string()
            }
            SceneDuReseau::Journee => {
                if self.nouveau(0)
                    && let Some((texte, date)) = &self.journal
                {
                    let evenements = crate::reseau::journee(texte, date);
                    println!("journée du {date} : {} outils", evenements.len() / 2);
                    self.reseau.rejouer(evenements, t);
                }
                "journee".to_string()
            }
            SceneDuReseau::Mesure => {
                let phase = if t < 10.0 {
                    "repos"
                } else if t < 25.0 {
                    "activite"
                } else if t < 40.0 {
                    "sujet"
                } else {
                    "tout"
                };
                self.reseau.demo(phase == "activite" || phase == "tout");
                self.voix = phase == "tout";
                let k = if t < 25.0 {
                    0
                } else if t < 40.0 {
                    1
                } else {
                    2 + ((t - 40.0) / 5.0).floor() as i64
                };
                if self.nouveau(k) {
                    match k {
                        0 => {}
                        1 => self.sujet_js(json!({ "s": "ecole", "d": 1.0e9 }), t),
                        _ => {
                            let image = IMAGES[k as usize % IMAGES.len()].0;
                            let c = couleurs[k as usize % 4];
                            self.sujet_js(json!({ "s": image, "c": c, "p": [0, 1, 2, 3], "d": 5000 }), t);
                        }
                    }
                }
                phase.to_string()
            }
        };
        let (dues, restent): (Vec<_>, Vec<_>) = std::mem::take(&mut self.fins).into_iter().partition(|(q, _)| *q <= t);
        self.fins = restent;
        for (q, e) in dues {
            self.reseau.recevoir(&e, q);
        }
        phase
    }

    /// Les touches de l'essai.
    fn touches(&mut self, ui: &egui::Ui, t: f64) {
        use crate::reseau::IMAGES;
        use egui::Key;
        let presse = |k: Key| ui.input(|i| i.key_pressed(k));
        if presse(Key::D) {
            let demo = self.reseau.modes().demo;
            self.reseau.demo(!demo);
        }
        if presse(Key::E) {
            let econome = self.reseau.modes().econome;
            self.reseau.econome(!econome);
        }
        if presse(Key::S) {
            self.image = (self.image + 1) % IMAGES.len();
            self.reseau.sujet(IMAGES[self.image].0, 15.0, t);
        }
        if presse(Key::C) {
            self.image += 1;
            let c = [[255, 200, 40], [90, 190, 255], [255, 122, 184]][self.image % 3];
            self.reseau.teinte(c, 10.0, t);
        }
        if presse(Key::V) {
            self.voix = !self.voix;
        }
        if presse(Key::P) {
            self.image += 1;
            let (outil, detail) = OUTILS_DES_PERSONNES[self.image % 4];
            self.outil(outil, detail, t);
        }
        if presse(Key::J)
            && let Some((texte, date)) = &self.journal
        {
            self.reseau.rejouer(crate::reseau::journee(texte, date), t);
        }
    }

    /// Autour du réseau, comme le tableau HTML (cdc:78-93) : le titre, la ligne d'état tapée, les compteurs et
    /// l'heure, la courbe d'activité, la légende et les personnes, le mode ; et ce que l'essai mesure.
    fn habiller(&self, ui: &egui::Ui, rect: egui::Rect, t: f64, phase: &str) {
        use egui::text::{LayoutJob, TextFormat};
        use egui::{Align2, Color32, FontId, Pos2, Stroke, StrokeKind, pos2, vec2};
        let p = ui.painter();
        let cyan = Color32::from_rgb(0, 229, 255);
        let vert = Color32::from_rgb(124, 255, 107);
        let blanc = Color32::from_rgb(232, 251, 255);
        let gris = Color32::from_rgb(95, 138, 168);
        let sombre = Color32::from_rgb(72, 107, 133);
        let format = |taille: f32, couleur: Color32, espace: f32| TextFormat {
            font_id: FontId::monospace(taille),
            color: couleur,
            extra_letter_spacing: espace,
            ..Default::default()
        };
        // Le titre.
        let mut job = LayoutJob::default();
        job.append("EVE", 0.0, format(15.0, cyan, 4.8));
        let sous_titre = format!("\n{} · ACTIVITÉ EN DIRECT · 360°", self.reseau.titre());
        job.append(&sous_titre, 0.0, format(11.0, gris, 1.3));
        p.galley(rect.min + vec2(20.0, 14.0), p.layout_job(job), Color32::PLACEHOLDER);
        // La ligne d'état : le texte tapé, puis le curseur (cyan) quand il se montre.
        let ligne = self.reseau.ligne_d_etat(t);
        let (texte, curseur) = match ligne.strip_suffix(crate::reseau::CURSEUR) {
            Some(x) => (x, true),
            None => (ligne.strip_suffix(' ').unwrap_or(&ligne), false),
        };
        let mut job = LayoutJob::default();
        job.append(texte, 0.0, format(15.0, blanc, 2.1));
        job.append(
            &crate::reseau::CURSEUR.to_string(),
            3.0,
            format(15.0, if curseur { cyan } else { Color32::TRANSPARENT }, 0.0),
        );
        let g = p.layout_job(job);
        p.galley(pos2(rect.center().x - g.size().x / 2.0, rect.top() + 62.0), g, Color32::PLACEHOLDER);
        // Les compteurs et l'heure, en haut à droite.
        let (total, en_cours) = self.reseau.compteurs();
        let droite = rect.right() - 20.0;
        p.text(pos2(droite, rect.top() + 12.0), Align2::RIGHT_TOP, total.to_string(), FontId::monospace(22.0), cyan);
        p.text(pos2(droite, rect.top() + 40.0), Align2::RIGHT_TOP, "ACTIONS", FontId::monospace(11.0), gris);
        p.text(pos2(droite, rect.top() + 56.0), Align2::RIGHT_TOP, en_cours.to_string(), FontId::monospace(22.0), vert);
        p.text(pos2(droite, rect.top() + 84.0), Align2::RIGHT_TOP, "EN COURS", FontId::monospace(11.0), gris);
        let heure = format!("{} · {}", crate::journal::heure(), crate::journal::date());
        p.text(
            pos2(droite, rect.top() + 102.0),
            Align2::RIGHT_TOP,
            heure,
            FontId::monospace(12.0),
            Color32::from_rgb(127, 180, 212),
        );
        // La courbe d'activité, en bas à gauche.
        let cases = self.reseau.activite(t);
        let (x0, y0, l, h) = (rect.left() + 20.0, rect.bottom() - 52.0, 360.0, 34.0);
        let cadre = egui::Rect::from_min_max(pos2(x0, y0 - h), pos2(x0 + l, y0));
        p.rect_stroke(cadre, 0.0, Stroke::new(1.0, cyan.gamma_multiply(0.25)), StrokeKind::Inside);
        let pas = l / (cases.len().max(2) - 1) as f32;
        let points: Vec<Pos2> =
            cases.iter().enumerate().map(|(i, v)| pos2(x0 + i as f32 * pas, y0 - v * (h - 4.0))).collect();
        p.add(egui::Shape::line(points.clone(), Stroke::new(5.0, cyan.gamma_multiply(0.12))));
        p.add(egui::Shape::line(points, Stroke::new(1.8, cyan)));
        p.text(
            pos2(x0, y0 - h - 3.0),
            Align2::LEFT_BOTTOM,
            "ACTIVITÉ · 63 DERNIÈRES SECONDES",
            FontId::monospace(10.0),
            sombre,
        );
        // La légende, les personnes, le mode.
        let mut x = rect.left() + 20.0;
        let y = rect.bottom() - 18.0;
        let pastille = |x: &mut f32, couleur: Color32, mot: &str| {
            p.circle_filled(pos2(*x + 4.0, y), 4.0, couleur);
            let r = p.text(pos2(*x + 14.0, y), Align2::LEFT_CENTER, mot, FontId::monospace(11.0), sombre);
            *x = r.right() + 18.0;
        };
        pastille(&mut x, cyan, "ENTRÉE (requête d'Eve)");
        pastille(&mut x, vert, "SORTIE (réponse reçue)");
        for personne in personnes_d_exemple() {
            let [r, v, b] = personne.couleur;
            pastille(&mut x, Color32::from_rgb(r, v, b), &personne.nom.to_uppercase());
        }
        let modes = self.reseau.modes();
        let mut mode = Vec::new();
        if modes.demo {
            mode.push("MODE DÉMO · D POUR ARRÊTER");
        }
        if modes.rediffusion {
            mode.push("REDIFFUSION DE LA JOURNÉE");
        }
        if modes.econome {
            mode.push(if modes.econome_de_lui_meme {
                "MODE ÉCONOME : LE RÉSEAU S'EST ALLÉGÉ"
            } else {
                "MODE ÉCONOME"
            });
        }
        if let Some(s) = self.reseau.sujet_en_cours() {
            mode.push(s);
        }
        p.text(pos2(x + 12.0, y), Align2::LEFT_CENTER, mode.join(" · "), FontId::monospace(11.0), gris);
        // Ce que l'essai mesure, en bas à droite.
        let cadence = match self.images.front() {
            Some(debut) if t > *debut => (self.images.len() - 1) as f64 / (t - debut),
            _ => 0.0,
        };
        let mesure = format!("{} · {phase} · {cadence:.0} images/s · dessiner {:.2} ms", self.nom, self.cout);
        p.text(pos2(droite, y), Align2::RIGHT_CENTER, mesure, FontId::monospace(11.0), sombre);
    }
}

impl eframe::App for EssaiDuReseau {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let t = ui.input(|i| i.time);
        if self.secondes.is_some_and(|s| t >= s) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.touches(ui, t);
        let phase = self.jouer(t);
        // La voix simulée : celle de l'essai de l'orbe (0,25 + 0,6 |sin(t / 170 ms)| × le hasard).
        let hasard = ((t * 12.9898).sin() * 43_758.545).rem_euclid(1.0);
        let niveau = if self.voix { (0.25 + 0.6 * (t * 1000.0 / 170.0).sin().abs() * hasard) as f32 } else { 0.0 };
        self.reseau.voix(niveau);
        self.images.push_back(t);
        while self.images.front().is_some_and(|x| t - x > 2.0) {
            self.images.pop_front();
        }
        let mut cout = 0.0;
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
            let rect = ui.max_rect();
            let debut = Instant::now();
            self.reseau.dessiner(ui, rect, t);
            cout = debut.elapsed().as_secs_f64() * 1000.0;
            if !self.nu {
                self.habiller(ui, rect, t, &phase);
            }
        });
        self.cout = cout;
        if self.mesure && t > 1.0 {
            let image = frame.info().cpu_usage.map_or(0.0, |s| f64::from(s) * 1000.0);
            match self.releves.iter_mut().find(|r| r.0 == phase) {
                Some(r) => {
                    r.1.push(cout);
                    r.2.push(image);
                }
                None => self.releves.push((phase, vec![cout], vec![image])),
            }
        }
        if let Some(dossier) = &self.captures {
            while let Some(&a) = self.aux.first()
                && t >= a
            {
                self.aux.remove(0);
                let chemin = dossier.join(format!("{}-{a}s.png", self.nom.replace(' ', "-")));
                ui.ctx().request_screenshot(move |image| match enregistrer_png(&image, &chemin) {
                    Ok(()) => println!("capture : {}", chemin.display()),
                    Err(e) => eprintln!("capture {} : {e}", chemin.display()),
                });
            }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.mesure {
            println!("phase            images   dessiner (médiane, p90)   image entière (médiane, p90)");
            for (phase, cout, image) in &self.releves {
                let p90 = |v: &[f64]| {
                    let mut v = v.to_vec();
                    v.sort_by(f64::total_cmp);
                    v.get(v.len() * 9 / 10).copied().unwrap_or(0.0)
                };
                println!(
                    "{phase:<16} {:>6}   {:>8.3} ms {:>8.3} ms     {:>8.3} ms {:>8.3} ms",
                    cout.len(),
                    mediane(cout).unwrap_or(0.0),
                    p90(cout),
                    mediane(image).unwrap_or(0.0),
                    p90(image)
                );
            }
        }
    }
}

/// Une capture de la fenêtre, en PNG.
fn enregistrer_png(image: &egui::ColorImage, chemin: &Path) -> std::io::Result<()> {
    ecrire_png(image, chemin, png::Compression::default())
}

/// Écrit `image` en PNG, à la compression voulue : `Fast` pour les images d'un film, qui s'écrivent à la cadence de
/// l'écran.
fn ecrire_png(image: &egui::ColorImage, chemin: &Path, compression: png::Compression) -> std::io::Result<()> {
    let fichier = std::io::BufWriter::new(std::fs::File::create(chemin)?);
    let mut png = png::Encoder::new(fichier, image.size[0] as u32, image.size[1] as u32);
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    png.set_compression(compression);
    let mut ecrire = png.write_header().map_err(std::io::Error::other)?;
    let octets: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    ecrire.write_image_data(&octets).map_err(std::io::Error::other)
}

// ---- La météo du centre de commande (le lot 6.2b) -----------------------------------------------------------------

/// `eve --essai-meteo [latitude longitude]` : une lecture de la météo par son fil, comme dans la fenêtre, pour
/// Montréal d'office (une grande ville publique : jamais le lieu d'une famille dans un essai). Elle imprime la
/// requête, le bulletin, et la tempête qu'il donnerait en hiver. Une seule requête.
pub fn meteo(args: &[String]) -> i32 {
    use crate::meteo::{self, Meteo, Tempete};
    let nombres: Vec<Option<f64>> = args.iter().map(|a| a.trim().replace(',', ".").parse().ok()).collect();
    let (ville, latitude, longitude) = match nombres.as_slice() {
        [] => ("Montréal", 45.50, -73.57),
        [Some(latitude), Some(longitude)] => ("", *latitude, *longitude),
        _ => {
            eprintln!("eve --essai-meteo [latitude longitude] : deux nombres (Montréal d'office : 45.50 -73.57)");
            return 2;
        }
    };
    // Le lieu passe par l'objet de la famille, comme dans la fenêtre.
    let famille = serde_json::json!({ "meteo": { "ville": ville, "latitude": latitude, "longitude": longitude } });
    let Some(lieu) = meteo::lieu_de(&famille) else {
        eprintln!("{latitude} {longitude} : pas un lieu (la latitude de -90 à 90, la longitude de -180 à 180)");
        return 2;
    };
    println!("requête     : {}", meteo::requete(&lieu));
    let t0 = Instant::now();
    let m = Meteo::lancer(lieu);
    let b = loop {
        if let Some(b) = m.bulletin() {
            break b;
        }
        if let Some(e) = m.panne() {
            eprintln!("pas de bulletin : {e}");
            return 1;
        }
        if t0.elapsed() > Duration::from_secs(120) {
            eprintln!("pas de bulletin en deux minutes");
            return 1;
        }
        thread::sleep(Duration::from_millis(50));
    };
    let l = m.lieu();
    let nom = if l.ville.is_empty() { "sans ville" } else { l.ville.as_str() };
    println!("lieu        : {nom} ({:.2}, {:.2}), fuseau {}", l.latitude, l.longitude, l.fuseau);
    println!("lu en       : {:.2} s", t0.elapsed().as_secs_f64());
    println!("ligne       : {}", b.ligne);
    println!("code        : {} (il neige : {})", b.code, if b.il_neige { "oui" } else { "non" });
    println!("température : {} °C", b.temperature);
    println!("aujourd'hui : {} cm de neige, des rafales de {} km/h au plus", b.neige_cm, b.rafales_kmh);
    // Le texte du panneau de la page (cdc:675), ses nombres arrondis.
    let panneau = |t: &Tempete| {
        format!("TEMPÊTE POSSIBLE · {} cm de neige · rafales {} km/h", t.neige_cm.round(), t.rafales_kmh.round())
    };
    let sans = "pas de tempête (de novembre à avril : 5 cm de neige, ou des rafales de 70 km/h)";
    println!("tempête     : {}", b.tempete.as_ref().map_or_else(|| sans.to_string(), panneau));
    let hiver = meteo::tempete_de(1, b.neige_cm, b.rafales_kmh);
    println!("en janvier  : {}", hiver.as_ref().map_or_else(|| sans.to_string(), panneau));
    0
}

// ---- Les panneaux du centre de commande (le lot 6) ----------------------------------------------------------------

/// Le fichier qui dit qu'un `activite\` est celui de l'essai des panneaux : l'essai le réécrit ; un autre se lit sans
/// que rien n'y soit écrit.
const MARQUE_DES_PANNEAUX: &str = "essai-panneaux.txt";

/// `eve --essai-panneaux [dossier] [heure] [--date AAAA-MM-JJ] [--tempete NEIGE,RAFALES | --sans-tempete] [--secours]
/// [--comptabilite | --comptabilite-de DOSSIER] [--secondes N] [--captures DOSSIER --aux T1,T2…]` : les panneaux du
/// centre de commande — le panneau d'information qui tourne, les échéances, la décision du jour — dans une fenêtre à
/// part, sur fond noir, posés comme dans la fenêtre native.
///
/// Le dossier (d'office `%TEMP%\eve-essai-panneaux`) reçoit des données inventées, datées d'après le jour de l'essai :
/// `activite\*.js` (une famille inventée, ses échéances, son budget…), `comptabilite\tableau\donnees.json` (une
/// comptabilité inventée) et `journal\eve.log` (les outils d'une journée : 18 actions, dont 4 web, 3 courriels, 6
/// fichiers). Un dossier dont le `activite\` vient d'ailleurs se lit tel quel, et rien n'y est écrit. L'horloge de
/// l'essai part du jour et de l'heure dits, puis avance (« 16:39:50 » : la cueillette inventée, de 16 h 40 à 17 h en
/// semaine, commence dix secondes plus tard) ; la météo inventée prévoit 14 cm de neige et des rafales de 72 km/h (la
/// tempête se montre de novembre à avril) ; `--secours` : la famille vient de la valeur de secours, sans `famille.js`.
/// `--comptabilite` : l'encaisse et les échéances fiscales viennent de la comptabilité inventée, comme le réglage
/// `comptabilite` le fait dans Eve (`budget.js` et les échéances « fiscal » d'`echeances.js` ne comptent plus) ;
/// `--comptabilite-de DOSSIER` : d'une autre, lue telle quelle. `--captures` enregistre la fenêtre en PNG aux secondes
/// dites (`--aux 3,11`) ; `--secondes N` la ferme d'elle-même, Échap aussi.
pub fn panneaux(args: &[String]) -> i32 {
    let valeur = |nom: &str| args.iter().position(|a| a == nom).and_then(|i| args.get(i + 1)).cloned();
    let a_valeur = ["--date", "--tempete", "--secondes", "--captures", "--aux", "--comptabilite-de"];
    let positionnels: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(k, a)| !a.starts_with("--") && (*k == 0 || !a_valeur.contains(&args[k - 1].as_str())))
        .map(|(_, a)| a)
        .collect();
    let dossier =
        positionnels.first().map_or_else(|| std::env::temp_dir().join("eve-essai-panneaux"), std::path::PathBuf::from);
    let date = valeur("--date").unwrap_or_else(crate::journal::date);
    let Some(jour) = crate::centre::numero_du_jour(&date) else {
        eprintln!("--date : « {date} » n'est pas une date (AAAA-MM-JJ)");
        return 2;
    };
    let maintenant = crate::journal::horodatage().get(11..).unwrap_or("12-00-00").replace('-', ":");
    let heure = positionnels.get(1).map_or(maintenant, |h| h.to_string());
    let Some(seconde) = seconde_du_jour(&heure) else {
        eprintln!("heure : « {heure} » n'est pas une heure (HH:MM ou HH:MM:SS)");
        return 2;
    };
    let tempete = match valeur("--tempete") {
        _ if args.iter().any(|a| a == "--sans-tempete") => None,
        None => Some((14.0, 72.0)),
        Some(v) => match v.split_once(',').and_then(|(n, r)| Some((n.trim().parse().ok()?, r.trim().parse().ok()?))) {
            Some(t) => Some(t),
            None => {
                eprintln!("--tempete : « {v} » n'est pas NEIGE,RAFALES (en centimètres et en km/h)");
                return 2;
            }
        },
    };
    let activite = dossier.join("activite");
    if activite.is_dir() && !activite.join(MARQUE_DES_PANNEAUX).is_file() {
        println!(
            "{} : des données qui ne sont pas de l'essai, lues telles quelles (rien n'y est écrit)",
            activite.display()
        );
    } else if let Err(e) = ecrire_les_panneaux_d_essai(&dossier, jour, seconde, args.iter().any(|a| a == "--secours")) {
        eprintln!("{} : {e}", dossier.display());
        return 1;
    } else {
        println!("données inventées : {}", dossier.display());
    }
    let meteo = tempete.map_or("aucune".to_string(), |(n, r)| format!("{n} cm de neige, rafales de {r} km/h"));
    println!("l'horloge de l'essai part du {date} à {heure} ; la météo : {meteo}");
    // La comptabilité, comme le réglage `comptabilite` la nomme dans Eve : l'inventée, une autre, ou aucune.
    let comptabilite = match valeur("--comptabilite-de") {
        Some(c) => Some(c),
        None if args.iter().any(|a| a == "--comptabilite") => {
            Some(dossier.join("comptabilite").to_string_lossy().into_owned())
        }
        None => None,
    };
    match &comptabilite {
        Some(c) => println!("la comptabilité : {c} (l'encaisse et les échéances fiscales en viennent)"),
        None => println!("sans comptabilité : l'encaisse et les échéances fiscales viennent d'activite\\"),
    }
    let captures = valeur("--captures").map(std::path::PathBuf::from);
    if let Some(c) = &captures
        && let Err(e) = std::fs::create_dir_all(c)
    {
        eprintln!("{} : {e}", c.display());
        return 1;
    }
    let mut aux: Vec<f64> =
        valeur("--aux").map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
    aux.sort_by(f64::total_cmp);
    let secondes = valeur("--secondes").and_then(|s| s.parse::<f64>().ok());
    let titre = "Eve — Essai des panneaux";
    let vue = egui::ViewportBuilder::default().with_title(titre).with_inner_size([1440.0, 900.0]);
    let options = eframe::NativeOptions { viewport: vue, ..Default::default() };
    let creer: eframe::AppCreator<'_> = Box::new(move |cc| {
        crate::natif::polices(&cc.egui_ctx);
        crate::natif::apparence(&cc.egui_ctx);
        let journal = dossier.join("journal");
        let centre = crate::centre::Centre::avec_journal(Some(dossier), Some(journal), cc.egui_ctx.clone());
        centre.famille_de_secours(famille_de_secours_d_essai(jour));
        centre.tempete(tempete);
        centre.suivre_la_comptabilite(comptabilite.as_deref().unwrap_or(""));
        Ok(Box::new(EssaiDesPanneaux {
            centre,
            comptabilite: comptabilite.is_some(),
            jour,
            seconde,
            secondes,
            captures,
            aux,
            cueillette: false,
            annoncee: false,
        }))
    });
    match eframe::run_native(titre, options, creer) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("la fenêtre ne s'ouvre pas : {e}");
            1
        }
    }
}

/// « 16:39 » ou « 16:39:50 » → la seconde du jour.
fn seconde_du_jour(h: &str) -> Option<i64> {
    let p: Vec<i64> = h.trim().split(':').map(|x| x.trim().parse().ok()).collect::<Option<_>>()?;
    let (hh, mm, ss) = match p[..] {
        [hh, mm] => (hh, mm, 0),
        [hh, mm, ss] => (hh, mm, ss),
        _ => return None,
    };
    ((0..24).contains(&hh) && (0..60).contains(&mm) && (0..60).contains(&ss)).then_some(hh * 3600 + mm * 60 + ss)
}

/// La famille de secours de l'essai, inventée : une personne (son anniversaire dans 15 jours), le site de l'école, sa
/// cueillette.
fn famille_de_secours_d_essai(jour: i64) -> Value {
    let fete = crate::centre::date_du_numero(jour + 15);
    let annee: i64 = fete[..4].parse().unwrap_or(2000);
    serde_json::json!({
        "personnes": [
            { "nom": "Sam", "couleur": [124, 255, 107], "anniversaire": format!("{}{}", annee - 30, &fete[4..]) }
        ],
        "ecole": { "site": "ecoles.exemple.ca" },
        "cueillette": { "debut": "16:40", "fin": "17:00", "texte": "Va chercher Sam à l'aréna" }
    })
}

/// Les données inventées de l'essai des panneaux (personne de vrai), datées d'après son jour (`jour`, son numéro) et
/// l'heure de son départ (`seconde`) ; `secours` : sans `famille.js`.
fn ecrire_les_panneaux_d_essai(dossier: &Path, jour: i64, seconde: i64, secours: bool) -> std::io::Result<()> {
    use serde_json::json;
    let activite = dossier.join("activite");
    std::fs::create_dir_all(&activite)?;
    std::fs::create_dir_all(dossier.join("journal"))?;
    let d = |k: i64| crate::centre::date_du_numero(jour + k);
    // Une naissance d'il y a `ans` ans, dont l'anniversaire tombe dans `k` jours.
    let ne = |ans: i64, k: i64| {
        let x = d(k);
        format!("{}{}", x[..4].parse::<i64>().unwrap_or(2000) - ans, &x[4..])
    };
    let hm = |s: i64| format!("{:02}:{:02}", s.clamp(0, 86_399) / 3600, s.clamp(0, 86_399) / 60 % 60);
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |x| x.as_secs());
    let ecrire = |nom: &str, texte: String| std::fs::write(activite.join(nom), texte);
    ecrire(
        MARQUE_DES_PANNEAUX,
        "Ces fichiers sont inventés par eve --essai-panneaux, qui les réécrit à chaque essai.\n".into(),
    )?;
    // Des littéraux JavaScript comme ceux qu'écrit le spécialiste (des clés nues, des apostrophes, des virgules en
    // trop, des commentaires) : l'essai éprouve aussi leur lecture en JSON5.
    let echeances = [
        (1, "Journée pédagogique · pas d\\'école", "ecole"),
        (6, "Acompte provisionnel", "fiscal"),
        (12, "Rendez-vous chez l\\'optométriste", "famille"),
        (21, "Inspection de la voiture", "auto"),
        (40, "Spectacle de fin d\\'année", "ecole"),
        (-3, "Une échéance passée", "famille"),
    ];
    let lignes: Vec<String> =
        echeances.iter().map(|(k, q, t)| format!("  {{ d:'{}', q:'{q}', t:'{t}' }},", d(*k))).collect();
    let texte =
        format!("// Les échéances d'une famille inventée.\nwindow.EVE_ECHEANCES = [\n{}\n];\n", lignes.join("\n"));
    ecrire("echeances.js", texte)?;
    let decision = "texte:'Réserver la gardienne pour samedi soir', source:'Essai des panneaux'";
    ecrire("decision.js", format!("window.EVE_DECISION = {{ date:'{}', avant:'{}', {decision} }};\n", d(0), d(2)))?;
    let budget = [
        "// Une période inventée : ses estimations, pas un solde.".to_string(),
        "window.EVE_BUDGET = {".to_string(),
        format!("  debut:'{}', fin:'{}',", d(-40), d(110)),
        "  manqueMensuel:[300, 500],".to_string(),
        format!("  remises:[ {{ d:'{}', m:1250, q:'Acompte' }}, {{ d:'{}', m:2400, q:'Solde' }}, ],", d(20), d(80)),
        "};\n".to_string(),
    ];
    ecrire("budget.js", budget.join("\n"))?;
    // La comptabilité inventée, de la forme de son `format-donnees.md` (celle que lit `--comptabilite`) : des échéances
    // longues comme celles d'une vraie, qui se raccourcissent ; deux remises le même jour, qui s'additionnent ; une
    // remise passée ; deux éléments mal écrits, laissés de côté.
    let tableau = dossier.join("comptabilite").join("tableau");
    std::fs::create_dir_all(&tableau)?;
    let donnees = json!({
        "version": 1,
        "misAJour": d(-2),
        "echeances": [
            { "date": d(4), "quoi": "Acompte provisionnel d'impôt (fédéral et Québec) ; à verser en ligne",
              "pourQui": "Alex" },
            { "date": d(11), "quoi": "Remise des retenues à la source de la société",
              "pourQui": "Société exemple inc." },
            { "date": d(33), "quoi": "Cotisation au régime d'épargne-retraite pour la déduire de l'année précédente, \
                                      selon les droits de l'avis de cotisation", "pourQui": "Alex et Sam" },
            { "date": d(-6), "quoi": "Une échéance passée", "pourQui": "Ménage" },
            { "date": "le mois prochain", "quoi": "Une date mal écrite", "pourQui": "Ménage" }
        ],
        "personnes": [
            { "nom": "Alex", "type": "adulte" },
            { "nom": "Société exemple inc.", "type": "entreprise", "remises": [
                { "nom": "Retenues de paie", "montant": 1480, "echeance": d(11), "detail": "" },
                { "nom": "TPS et TVQ", "montant": 2150, "echeance": d(70) },
                { "nom": "Impôt de la société", "montant": 640, "echeance": d(70) },
                { "nom": "Une remise passée", "montant": 900, "echeance": d(-6) },
                { "nom": "Sans montant", "echeance": d(30) }
            ] }
        ]
    });
    std::fs::write(tableau.join("donnees.json"), serde_json::to_string_pretty(&donnees).unwrap_or_default())?;
    let questions = [
        "Heure du cours de natation ?",
        "Allergies à déclarer pour la sortie",
        "Budget du cadeau d\\'anniversaire",
        "Qui garde le chat en décembre ?",
    ];
    ecrire("questions.js", format!("window.EVE_QUESTIONS = [ '{}' ];\n", questions.join("', '")))?;
    let articles = [
        ("lait", false),
        ("pain de blé", false),
        ("pommes", true),
        ("café", false),
        ("riz basmati", false),
        ("yogourt", false),
        ("bananes", true),
    ];
    let items: Vec<String> = articles.iter().map(|(n, ok)| format!("{{ n:'{n}', ok:{ok} }}")).collect();
    ecrire("epicerie.js", format!("window.EVE_EPICERIE = {{ maj:'{}', items:[ {} ] }};\n", d(0), items.join(", ")))?;
    let souper = "menu:'Lasagne aux légumes', sortir:'la sauce tomate du congélateur'";
    ecrire("souper.js", format!("window.EVE_SOUPER = {{ date:'{}', {souper} }};\n", d(0)))?;
    // Les courriels et l'agenda, en vrai JSON précédé d'une marque d'ordre des octets, comme les écrit le script
    // d'Outlook ; leur âge se compte en vraie heure (dix minutes plus tard, la liaison est absente).
    let boites = json!({ "t": unix, "boites": [
        { "nom": "maison@exemple.ca", "non_lus": 3, "ecole": true },
        { "nom": "bureau@exemple.ca", "non_lus": 0, "ecole": false }
    ] });
    let agenda = json!({ "t": unix, "rdv": [
        { "d": d(0), "h": hm(seconde - 3600), "titre": "Commencé il y a une heure" },
        { "d": d(0), "h": hm(seconde + 75 * 60), "titre": "Cours de natation" },
        { "d": d(0), "h": "", "titre": "Collecte des feuilles" },
        { "d": d(1), "h": "09:00", "titre": "Réunion de parents" },
        { "d": d(1), "h": "18:30", "titre": "Souper chez les voisins" }
    ] });
    ecrire("outlook.js", format!("\u{feff}window.EVE_OUTLOOK={boites};\nwindow.EVE_AGENDA={agenda};\n"))?;
    let famille = activite.join("famille.js");
    if secours {
        if famille.exists() {
            std::fs::remove_file(&famille)?;
        }
    } else {
        ecrire(
            "famille.js",
            format!(
                "// Une famille inventée pour l'essai des panneaux : personne de vrai.\nwindow.EVE_FAMILLE = {{\n  \
                 personnes: [\n    {{ nom: 'Marc', couleur: [0, 229, 255], motifs: ['chantier'], anniversaire: '{}' \
                 }},\n    {{ nom: 'Julie', couleur: '#ff7ab8' }},\n    {{ nom: 'Léa', couleur: [255, 210, 63], \
                 anniversaire: '{}' }},\n    {{ nom: 'Noah', motifs: ['garderie'], anniversaire: '{}' }}, // la \
                 couleur de son rang\n  ],\n  ecole: {{ motifs: ['les castors'], site: 'ecoles.exemple.ca', conge_de: \
                 'Léa' }},\n  meteo: {{ ville: 'Exempleville', latitude: 46.0, longitude: -72.0, fuseau: \
                 'America/Toronto' }},\n  cueillette: {{ jours: [1, 2, 3, 4, 5], debut: '16:40', fin: '17:00',\n    \
                 texte: 'Va chercher Noah au service de garde', note: 'RETARD : 2 $ LA MINUTE' }},\n}};\n",
                &d(100)[5..],
                ne(6, 40),
                ne(3, 9)
            ),
        )?;
    }
    // Le journal d'une journée d'Eve : ses lignes « outil ▸ », celles de la veille, qui ne comptent pas, et ce qui
    // n'est pas un outil.
    let (veille, du_jour, ici) = (d(-1), d(0), dossier.display());
    let journal = [
        format!("{veille} 22:10:00 outil ▸ Read : {ici}\\notes.md"),
        format!("{veille} 23:59:59 outil ▸ WebSearch"),
        format!("{du_jour} 07:58:02 Eve 0.1.0 démarre (essai des panneaux)."),
        format!("{du_jour} 07:58:12 outil ▸ Read : {ici}\\activite\\echeances.js"),
        format!("{du_jour} 07:58:15 outil ▸ Glob : activite/*.js"),
        format!("{du_jour} 08:02:40 outil ▸ PowerShell : $o = New-Object -ComObject Outlook.Application"),
        format!("{du_jour} 08:10:00 outil ▸ WebSearch"),
        format!("{du_jour} 08:10:04 outil ▸ WebFetch"),
        format!("{du_jour} 08:11:30 outil ▸ Edit : {ici}\\activite\\epicerie.js"),
        format!("{du_jour} 09:15:00 outil ▸ Skill : courriels"),
        format!("{du_jour} 09:15:30 outil ▸ Write : {ici}\\menus\\semaine.md"),
        format!("{du_jour} 10:20:00 outil ▸ Bash : python scripts/meteo.py"),
        format!("{du_jour} 10:21:00 outil ▸ mcp__claude-in-chrome__navigate"),
        format!("{du_jour} 11:00:00 outil ▸ Grep : anniversaire"),
        format!("{du_jour} 11:30:00 outil ▸ PowerShell : Get-Date"),
        format!("{du_jour} 12:05:00 outil ▸ mcp__exemple__lister"),
        format!(
            "{du_jour} 12:05:01 outil rejeté avant la garde (rien n'a tourné) : mcp__exemple__lister (toolu_essai)"
        ),
        format!("{du_jour} 13:40:00 outil ▸ Agent : vérifier le calendrier scolaire"),
        format!("{du_jour} 14:00:00 outil ▸ Read : D:\\Exemple\\Comptes\\releve.csv"),
        format!("{du_jour} 15:00:00 outil ▸ PowerShell : Outlook.Application, les rendez-vous de demain"),
        format!("{du_jour} 16:00:00 outil ▸ WebSearch"),
        format!("{du_jour} 16:20:00 outil ▸ PowerShell : MAPI, la boîte de réception"),
    ];
    std::fs::write(dossier.join("journal").join("eve.log"), journal.join("\n") + "\n")
}

struct EssaiDesPanneaux {
    centre: crate::centre::Centre,
    /// Une comptabilité est nommée : l'encaisse et les échéances fiscales en viennent.
    comptabilite: bool,
    /// L'horloge de l'essai à son départ : le numéro du jour, la seconde du jour.
    jour: i64,
    seconde: i64,
    secondes: Option<f64>,
    captures: Option<std::path::PathBuf>,
    aux: Vec<f64>,
    /// La cueillette avait lieu à l'image d'avant : son front montant s'annonce, comme la fenêtre le verrait.
    cueillette: bool,
    /// La famille est annoncée à la console (une fois que le fil l'a lue).
    annoncee: bool,
}

impl EssaiDesPanneaux {
    /// D'où vient la famille, et ce que le réseau en prendrait : les personnes, les mots de l'école.
    fn annoncer(&self) {
        let source = if self.centre.valeur("EVE_FAMILLE").is_null() { "de secours" } else { "de famille.js" };
        let personnes: Vec<String> = self
            .centre
            .personnes()
            .iter()
            .map(|p| {
                let motifs = if p.motifs.is_empty() { String::new() } else { format!(" ({})", p.motifs.join(", ")) };
                format!("{} {:?}{motifs}", p.nom, p.couleur)
            })
            .collect();
        println!(
            "famille {source} : {} ; les mots de l'école : {}",
            personnes.join(" · "),
            self.centre.motifs_de_l_ecole().join(", ")
        );
    }
}

impl eframe::App for EssaiDesPanneaux {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        use egui::{Align, Align2, Color32, FontId, Layout, UiBuilder, pos2, vec2};
        let t = ui.input(|i| i.time);
        if self.secondes.is_some_and(|s| t >= s) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // L'horloge de l'essai : son départ, plus le temps écoulé.
        let s = self.seconde + t as i64;
        let aujourdhui = crate::centre::date_du_numero(self.jour + s.div_euclid(86_400));
        let s = s.rem_euclid(86_400);
        let heure = format!("{:02}:{:02}", s / 3600, s / 60 % 60);
        let horloge = format!("{heure}:{:02}", s % 60);
        let cueillette = self.centre.cueillette(&aujourdhui, &heure);
        if cueillette && !self.cueillette {
            println!(
                "{aujourdhui} {horloge} : la cueillette commence — la fenêtre jouerait ses deux notes \
                 (Musique::aller) et ferait sonner la destination de la famille"
            );
        }
        self.cueillette = cueillette;
        if !self.annoncee && t > 2.5 {
            self.annoncee = true;
            self.annoncer();
        }
        let alerte = self.centre.alerte_famille(&aujourdhui, &heure);
        let (cyan, gris, sombre, clair) = (
            Color32::from_rgb(0, 229, 255),
            Color32::from_rgb(95, 138, 168),
            Color32::from_rgb(72, 107, 133),
            Color32::from_rgb(217, 243, 255),
        );
        egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::from_rgb(3, 6, 13))).show(ui, |ui| {
            let rect = ui.max_rect();
            let p = ui.painter().clone();
            p.text(
                rect.left_top() + vec2(20.0, 16.0),
                Align2::LEFT_TOP,
                "E V E · ESSAI DES PANNEAUX",
                FontId::monospace(14.0),
                cyan,
            );
            p.text(
                rect.left_top() + vec2(20.0, 40.0),
                Align2::LEFT_TOP,
                "DONNÉES INVENTÉES · ÉCHAP POUR FERMER",
                FontId::monospace(10.0),
                gris,
            );
            // L'horloge de l'essai, d'où vient la famille, et la destination de la famille qui sonnerait.
            let droite = pos2(rect.right() - 20.0, rect.top() + 16.0);
            p.text(droite, Align2::RIGHT_TOP, format!("{aujourdhui} · {horloge}"), FontId::monospace(14.0), clair);
            let source = if self.centre.valeur("EVE_FAMILLE").is_null() { "FAMILLE DE SECOURS" } else { "FAMILLE.JS" };
            let argent = if self.comptabilite { "L'ARGENT : LA COMPTABILITÉ" } else { "L'ARGENT : ACTIVITE\\" };
            p.text(
                droite + vec2(0.0, 24.0),
                Align2::RIGHT_TOP,
                format!("{source} · {argent}"),
                FontId::monospace(10.0),
                gris,
            );
            if alerte {
                let rouge = Color32::from_rgb(255, 107, 122);
                let texte = "LA DESTINATION FAMILLE SONNE";
                p.text(droite + vec2(0.0, 42.0), Align2::RIGHT_TOP, texte, FontId::monospace(11.0), rouge);
            }
            // Comme la fenêtre native : les échéances en haut à gauche ; le panneau d'information en bas à gauche, et
            // la décision du jour en bas, au centre (720 points au plus, à 128 points du bas comme la page), posés par
            // leur bas à la hauteur qu'ils avaient à l'image d'avant.
            let gauche =
                egui::Rect::from_min_size(rect.left_top() + vec2(20.0, 92.0), vec2(290.0, rect.height() * 0.45));
            ui.scope_builder(UiBuilder::new().max_rect(gauche), |ui| self.centre.echeances_hud(ui, &aujourdhui, t));
            let (id_panneau, id_decision) =
                (egui::Id::unique("hauteur-du-panneau-d-essai"), egui::Id::unique("hauteur-de-la-decision-d-essai"));
            let (h_panneau, h_decision) = ui.ctx().data(|d| {
                (d.get_temp::<f32>(id_panneau).unwrap_or(240.0), d.get_temp::<f32>(id_decision).unwrap_or(90.0))
            });
            let bas = rect.bottom() - 100.0;
            let panneau =
                egui::Rect::from_min_max(pos2(rect.left() + 20.0, bas - h_panneau), pos2(rect.left() + 330.0, bas));
            let h1 = ui
                .scope_builder(UiBuilder::new().max_rect(panneau), |ui| {
                    self.centre.panneau_hud(ui, &aujourdhui, &heure, t)
                })
                .response
                .rect
                .height();
            let (de, a) = (rect.left() + 300.0, rect.right() - 14.0);
            let large = (a - de).min(720.0);
            let milieu = (de + a) / 2.0;
            let bas = rect.bottom() - 128.0;
            let decision =
                egui::Rect::from_min_max(pos2(milieu - large / 2.0, bas - h_decision), pos2(milieu + large / 2.0, bas));
            let h2 = ui
                .scope_builder(UiBuilder::new().max_rect(decision).layout(Layout::top_down(Align::Center)), |ui| {
                    self.centre.decision_hud(ui, &aujourdhui, t)
                })
                .response
                .rect
                .height();
            if (h1 - h_panneau).abs() > 0.5 || (h2 - h_decision).abs() > 0.5 {
                ui.ctx().data_mut(|d| {
                    d.insert_temp(id_panneau, h1);
                    d.insert_temp(id_decision, h2);
                });
                ui.ctx().request_repaint();
            }
            // Les personnes de la famille, comme la légende de la page (cdc:92), d'après EVE_FAMILLE.
            let (mut x, y) = (rect.left() + 20.0, rect.bottom() - 18.0);
            for personne in self.centre.personnes() {
                let [r, v, b] = personne.couleur;
                p.circle_filled(pos2(x + 4.0, y), 4.0, Color32::from_rgb(r, v, b));
                let nom = personne.nom.to_uppercase();
                x = p.text(pos2(x + 14.0, y), Align2::LEFT_CENTER, nom, FontId::monospace(11.0), sombre).right() + 18.0;
            }
        });
        if let Some(dossier) = &self.captures {
            while let Some(&a) = self.aux.first()
                && t >= a
            {
                self.aux.remove(0);
                let chemin = dossier.join(format!("panneaux-{a}s.png"));
                ui.ctx().request_screenshot(move |image| match enregistrer_png(&image, &chemin) {
                    Ok(()) => println!("capture : {}", chemin.display()),
                    Err(e) => eprintln!("capture {} : {e}", chemin.display()),
                });
            }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
}

// ---- L'ambiance du centre de commande (le lot 6) ------------------------------------------------------------------

/// Les scènes de `eve --essai-ambiance`.
const SCENES_DE_L_AMBIANCE: &str = "tout, halloween, neige, noel, nuit, fete, veille, mesure";

/// Une phase de l'essai de l'ambiance : son nom, sa durée, le jour et l'heure qu'elle montre, la météo (il neige), la
/// fête (celle d'une personne d'exemple), la veille (qui vient deux secondes après son début) ; ou rien du tout (la
/// fenêtre sans l'ambiance : ce qu'elle coûte sans elle).
#[derive(Clone, Copy)]
struct PhaseDAmbiance {
    nom: &'static str,
    duree: f64,
    date: &'static str,
    heure: u32,
    il_neige: bool,
    fete: bool,
    veille: bool,
    rien: bool,
}

impl PhaseDAmbiance {
    const fn de(nom: &'static str, duree: f64, date: &'static str, heure: u32) -> PhaseDAmbiance {
        PhaseDAmbiance { nom, duree, date, heure, il_neige: false, fete: false, veille: false, rien: false }
    }
}

/// Les phases d'une scène, qui reviennent en boucle (celles de la mesure ne passent qu'une fois) : l'Halloween d'un
/// 24 octobre, la neige d'un 15 janvier (et d'un jour d'avril où il neige), Noël au soir, une nuit (la fenêtre ouverte
/// le jour, puis 23 h, puis le matin : le voile vient et s'en va), une fête un jour sans saison, la veille sur les
/// braises ; et pour la mesure, la fenêtre sans l'ambiance, chaque phase, puis tout à la fois (les braises, une fête,
/// la nuit et la veille).
fn phases_de_l_ambiance(scene: &str) -> Option<Vec<PhaseDAmbiance>> {
    let p = PhaseDAmbiance::de;
    let halloween = p("halloween", 8.0, "2026-10-24", 14);
    let neige = p("neige", 8.0, "2027-01-15", 14);
    let noel = p("noel", 8.0, "2026-12-24", 19);
    let nuit = p("nuit", 8.0, "2026-06-15", 23);
    let fete = PhaseDAmbiance { fete: true, ..p("fete", 8.0, "2026-09-15", 15) };
    let veille = PhaseDAmbiance { veille: true, ..p("veille", 10.0, "2026-10-24", 14) };
    let six = |x: PhaseDAmbiance| PhaseDAmbiance { duree: 6.0, ..x };
    Some(match scene {
        "tout" => vec![halloween, neige, noel, nuit, fete, veille],
        "halloween" => vec![halloween],
        // La neige de l'hiver, puis celle que la météo annonce un jour d'avril.
        "neige" => vec![neige, PhaseDAmbiance { il_neige: true, ..p("il neige", 8.0, "2027-04-10", 14) }],
        "noel" => vec![noel],
        "nuit" => {
            vec![
                p("jour", 3.0, "2026-06-15", 20),
                PhaseDAmbiance { duree: 10.0, ..nuit },
                p("matin", 7.0, "2026-06-15", 6),
            ]
        }
        "fete" => vec![fete],
        "veille" => vec![veille],
        "mesure" => vec![
            PhaseDAmbiance { rien: true, ..p("rien", 6.0, "2026-06-15", 14) },
            six(halloween),
            six(neige),
            six(noel),
            six(nuit),
            six(fete),
            six(veille),
            PhaseDAmbiance { fete: true, veille: true, ..p("ensemble", 8.0, "2026-10-24", 23) },
        ],
        _ => return None,
    })
}

/// Une famille d'exemple (personne de vrai) : Hugo, sans couleur (il prend celle de son quartier), et Zoé, née un 15
/// septembre, qui a la sienne.
fn famille_d_essai() -> Value {
    serde_json::json!({ "personnes": [
        { "nom": "Hugo", "anniversaire": "03-07" },
        { "nom": "Zoé", "couleur": [255, 122, 184], "anniversaire": "2019-09-15" },
    ] })
}

/// `eve --essai-ambiance [scène] [--date AAAA-MM-JJ] [--heure HH[:MM]] [--reseau] [--econome] [--mesure]
/// [--secondes N] [--captures DOSSIER --aux T1,T2…]` : l'ambiance du centre de commande (la saison, la fête, le
/// balayage, la nuit) et son écran de veille, sur un fond noir où quelques points figurent le réseau (`--reseau` : le
/// vrai). La ligne d'état tape le souhait d'anniversaire. `--date` et `--heure` forcent le moment de toutes les
/// phases ; `--econome`, le mode économe ; `--mesure` relève, phase par phase, ce que coûte l'ambiance sur le
/// processeur (`dessiner` et `veille`) et l'image entière selon eframe, et l'écrit en partant ; `--captures` enregistre
/// la fenêtre en PNG aux secondes dites ; `--secondes N` la ferme d'elle-même. La fenêtre s'ouvre sans prendre la main
/// au clavier ; la souris chasse la veille ; Échap la ferme.
pub fn ambiance(args: &[String]) -> i32 {
    let valeur = |nom: &str| args.iter().position(|a| a == nom).and_then(|i| args.get(i + 1)).cloned();
    let a_valeur = ["--date", "--heure", "--secondes", "--captures", "--aux"];
    let positionnels: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(k, a)| !a.starts_with("--") && (*k == 0 || !a_valeur.contains(&args[k - 1].as_str())))
        .map(|(_, a)| a)
        .collect();
    let nom = positionnels.first().map_or("tout", |s| s.as_str()).to_string();
    let Some(phases) = phases_de_l_ambiance(&nom) else {
        eprintln!("scène inconnue : {nom} ({SCENES_DE_L_AMBIANCE})");
        return 2;
    };
    let heure = match valeur("--heure") {
        Some(h) => {
            let mut p = h.split(':');
            let lire = |x: Option<&str>, max: u32| x.unwrap_or("0").trim().parse::<u32>().ok().filter(|v| *v <= max);
            match (lire(p.next(), 23), lire(p.next(), 59)) {
                (Some(h), Some(m)) => Some((h, m)),
                _ => {
                    eprintln!("--heure {h} : une heure, HH ou HH:MM");
                    return 2;
                }
            }
        }
        None => None,
    };
    let mesure = nom == "mesure" || args.iter().any(|a| a == "--mesure");
    let total: f64 = phases.iter().map(|p| p.duree).sum();
    let secondes = valeur("--secondes").and_then(|s| s.parse::<f64>().ok()).or((nom == "mesure").then_some(total));
    let econome = args.iter().any(|a| a == "--econome");
    let reseau = args.iter().any(|a| a == "--reseau");
    let captures = valeur("--captures").map(std::path::PathBuf::from);
    let mut aux: Vec<f64> =
        valeur("--aux").map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
    aux.sort_by(f64::total_cmp);
    let titre = "Eve — Essai de l'ambiance";
    // Elle s'ouvre sans prendre la main au clavier : on peut la lancer sans déranger ce qu'on fait.
    let vue = egui::ViewportBuilder::default().with_title(titre).with_inner_size([1400.0, 860.0]).with_active(false);
    let options = eframe::NativeOptions { viewport: vue, ..Default::default() };
    let date = valeur("--date");
    let creer: eframe::AppCreator<'_> = Box::new(move |cc| {
        crate::natif::polices(&cc.egui_ctx);
        crate::natif::apparence(&cc.egui_ctx);
        // Un dossier de travail d'exemple, pour nommer les destinations : rien n'y est lu ni écrit.
        let reseau = reseau.then(|| {
            let mut r = crate::reseau::Reseau::nouveau(Path::new(r"C:\Essai"));
            r.habiller(true, false);
            r.econome(econome);
            r
        });
        Ok(Box::new(EssaiDeLAmbiance {
            ambiance: crate::ambiance::Ambiance::nouvelle(),
            reseau,
            boucle: nom != "mesure",
            nom,
            phases,
            date,
            heure,
            econome,
            famille: famille_d_essai(),
            secondes,
            mesure,
            captures,
            aux,
            vue: None,
            ligne: None,
            releves: Vec::new(),
            images: VecDeque::new(),
            cout: 0.0,
        }))
    });
    match eframe::run_native(titre, options, creer) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("la fenêtre ne s'ouvre pas : {e}");
            1
        }
    }
}

struct EssaiDeLAmbiance {
    ambiance: crate::ambiance::Ambiance,
    /// Le vrai réseau (`--reseau`), sinon quelques points qui le figurent.
    reseau: Option<crate::reseau::Reseau>,
    nom: String,
    phases: Vec<PhaseDAmbiance>,
    boucle: bool,
    /// La date et l'heure qui remplacent celles des phases (`--date`, `--heure`).
    date: Option<String>,
    heure: Option<(u32, u32)>,
    econome: bool,
    famille: Value,
    secondes: Option<f64>,
    mesure: bool,
    captures: Option<std::path::PathBuf>,
    aux: Vec<f64>,
    /// Le rang de la phase en cours : sa veille se déclenche quand elle commence.
    vue: Option<usize>,
    /// La phrase que la ligne d'état tape (le souhait), et depuis quand.
    ligne: Option<(String, f64)>,
    /// Par phase : son nom, ce qu'a coûté l'ambiance, ce qu'a coûté l'image entière (ms).
    releves: Vec<(String, Vec<f64>, Vec<f64>)>,
    /// Les instants des images des deux dernières secondes (la cadence), et ce qu'a coûté l'ambiance à la dernière.
    images: VecDeque<f64>,
    cout: f64,
}

impl EssaiDeLAmbiance {
    /// La phase de l'instant `t`, son rang, et l'instant où elle a commencé.
    fn phase_a(&self, t: f64) -> (usize, PhaseDAmbiance, f64) {
        let tour: f64 = self.phases.iter().map(|p| p.duree).sum();
        let mut dans = if self.boucle { t.rem_euclid(tour) } else { t.min(tour) };
        for (k, p) in self.phases.iter().enumerate() {
            if dans < p.duree {
                return (k, *p, t - dans);
            }
            dans -= p.duree;
        }
        let k = self.phases.len() - 1;
        (k, self.phases[k], t)
    }

    /// La date que montre une phase : celle de `--date`, sinon la sienne.
    fn date_de<'a>(&'a self, phase: &'a PhaseDAmbiance) -> &'a str {
        self.date.as_deref().unwrap_or(phase.date)
    }

    /// Autour de l'ambiance, comme le tableau HTML (cdc:78-80) : le titre, la ligne d'état (le souhait tapé lettre à
    /// lettre, sinon la saison et le moment de la phase), l'aide, et ce que l'essai mesure.
    fn habiller(&self, ui: &egui::Ui, rect: egui::Rect, t: f64, phase: &PhaseDAmbiance, date: &str, heure: &str) {
        use egui::text::{LayoutJob, TextFormat};
        use egui::{Align2, Color32, FontId, pos2, vec2};
        let p = ui.painter();
        let cyan = Color32::from_rgb(0, 229, 255);
        let blanc = Color32::from_rgb(232, 251, 255);
        let gris = Color32::from_rgb(95, 138, 168);
        let format = |taille: f32, couleur: Color32, espace: f32| TextFormat {
            font_id: FontId::monospace(taille),
            color: couleur,
            extra_letter_spacing: espace,
            ..Default::default()
        };
        let mut job = LayoutJob::default();
        job.append("EVE", 0.0, format(15.0, cyan, 4.8));
        job.append("\nESSAI DE L'AMBIANCE · SAISON, FÊTE, BALAYAGE, NUIT, VEILLE", 0.0, format(11.0, gris, 1.3));
        p.galley(rect.min + vec2(20.0, 14.0), p.layout_job(job), Color32::PLACEHOLDER);
        // La ligne d'état (cdc:19-20, 469-473) : une lettre toutes les 22 ms, et le curseur qui clignote.
        let (texte, curseur) = match &self.ligne {
            Some((phrase, depuis)) if t - depuis < 8.0 => {
                let n = ((t - depuis) / 0.022) as usize;
                (phrase.chars().take(n).collect(), (t / 0.8).fract() < 0.5)
            }
            _ => {
                use crate::ambiance::Saison;
                let saison = match crate::ambiance::saison(self.date_de(phase), phase.il_neige) {
                    Some(Saison::Halloween) => "HALLOWEEN",
                    Some(Saison::Noel) => "NOËL",
                    Some(Saison::Neige) => "NEIGE",
                    None => "SANS SAISON",
                };
                let nuit = !(6..21).contains(&self.heure.map_or(phase.heure, |(h, _)| h));
                (format!("{saison} · {date} · {heure}{}", if nuit { " · NUIT" } else { "" }), false)
            }
        };
        let mut job = LayoutJob::default();
        job.append(&texte, 0.0, format(15.0, blanc, 2.1));
        job.append("▌", 3.0, format(15.0, if curseur { cyan } else { Color32::TRANSPARENT }, 0.0));
        let g = p.layout_job(job);
        p.galley(pos2(rect.center().x - g.size().x / 2.0, rect.top() + 62.0), g, Color32::PLACEHOLDER);
        let y = rect.bottom() - 18.0;
        let aide = "ÉCHAP : FERMER · LA SOURIS CHASSE LA VEILLE";
        p.text(pos2(rect.left() + 20.0, y), Align2::LEFT_CENTER, aide, FontId::monospace(11.0), gris);
        let cadence = match self.images.front() {
            Some(debut) if t > *debut => (self.images.len() - 1) as f64 / (t - debut),
            _ => 0.0,
        };
        let mesure = format!("{} · {} · {cadence:.0} images/s · ambiance {:.2} ms", self.nom, phase.nom, self.cout);
        p.text(pos2(rect.right() - 20.0, y), Align2::RIGHT_CENTER, mesure, FontId::monospace(11.0), gris);
    }
}

/// Quelques points pour figurer le réseau : une sphère de Fibonacci de 140 points qui tourne lentement, plus vive
/// devant, et le cercle d'Eve au milieu.
fn points_du_reseau(p: &egui::Painter, rect: egui::Rect, t: f64) {
    use egui::{Color32, Stroke, pos2};
    let centre = rect.center() + egui::vec2(0.0, 4.0);
    let echelle = rect.width().min(rect.height()) * 0.36;
    let lacet = (t * 0.024).rem_euclid(std::f64::consts::TAU) as f32;
    for k in 0..140 {
        let y = 1.0 - 2.0 * (k as f32 + 0.5) / 140.0;
        let r = (1.0 - y * y).sqrt();
        let (s, c) = (k as f32 * 2.399_963 + lacet).sin_cos();
        let (x, z) = (c * r, s * r);
        let loin = 3.0 / (3.0 + z);
        let vif = (40.0 + 110.0 * (1.0 - (z + 1.0) / 2.0)) as u8;
        let point = pos2(centre.x + x * echelle * loin, centre.y + y * echelle * loin);
        p.circle_filled(point, 1.6 * loin, Color32::from_rgb_additive(0, vif, vif.saturating_add(16)));
    }
    p.circle_filled(centre, 30.0, Color32::from_rgb(4, 16, 30));
    p.circle_stroke(centre, 30.0, Stroke::new(2.4, Color32::from_rgb(0, 229, 255)));
}

impl eframe::App for EssaiDeLAmbiance {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        use crate::ambiance::{self, Moment, Veille};
        let t = ui.input(|i| i.time);
        if self.secondes.is_some_and(|s| t >= s) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let (k, phase, debut) = self.phase_a(t);
        if self.vue != Some(k) {
            self.vue = Some(k);
            // La veille d'une phase vient deux secondes après son début : sa dernière activité, 88 s plus tôt.
            if phase.veille {
                self.ambiance.signal(debut + 2.0 - 90.0);
            }
        }
        let date = self.date_de(&phase).to_string();
        let (heure, minute) = self.heure.unwrap_or((phase.heure, 5));
        // La fête d'une phase de fête : celle de la personne d'exemple née un 15 septembre ; sinon celle du jour.
        let fete = ambiance::fete_du_jour(&self.famille, if phase.fete { "2026-09-15" } else { date.as_str() });
        let econome = self.econome || self.reseau.as_ref().is_some_and(|r| r.modes().econome);
        self.images.push_back(t);
        while self.images.front().is_some_and(|x| t - x > 2.0) {
            self.images.pop_front();
        }
        let mut cout = 0.0;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(egui::Color32::BLACK)).show(ui, |ui| {
            let rect = ui.max_rect();
            match self.reseau.as_mut() {
                Some(r) => {
                    r.dessiner(ui, rect, t);
                }
                None => points_du_reseau(ui.painter(), rect, t),
            }
            let horodatage = format!("{date} {heure:02}-{minute:02}-00");
            let (h, d) = (ambiance::heure_de_veille(&horodatage), ambiance::date_longue(&date));
            let e = ambiance::prochaine_echeance(3, "Rapport d'essai à remettre");
            let contenu = Veille { heure: &h, date: &d, meteo: "VILLE D'ESSAI · 4 °C · COUVERT", echeance: &e };
            let chrono = Instant::now();
            if !phase.rien {
                let moment = Moment { date: &date, heure, econome, il_neige: phase.il_neige, fete: fete.as_ref() };
                self.ambiance.dessiner(ui.painter(), rect, t, &moment);
                if let Some(souhait) = self.ambiance.souhait(t) {
                    self.ligne = Some((souhait, t));
                }
            }
            cout = chrono.elapsed().as_secs_f64() * 1000.0;
            self.habiller(ui, rect, t, &phase, &d, &h);
            if !phase.rien {
                let chrono = Instant::now();
                self.ambiance.veille(ui.painter(), rect, t, !phase.veille, &contenu);
                cout += chrono.elapsed().as_secs_f64() * 1000.0;
            }
        });
        self.cout = cout;
        if self.mesure && t > 1.0 {
            let image = frame.info().cpu_usage.map_or(0.0, |s| f64::from(s) * 1000.0);
            let nom = phase.nom.to_string();
            match self.releves.iter_mut().find(|r| r.0 == nom) {
                Some(r) => {
                    r.1.push(cout);
                    r.2.push(image);
                }
                None => self.releves.push((nom, vec![cout], vec![image])),
            }
        }
        if let Some(dossier) = &self.captures {
            while let Some(&a) = self.aux.first()
                && t >= a
            {
                self.aux.remove(0);
                let chemin = dossier.join(format!("ambiance-{}-{a}s.png", self.nom));
                ui.ctx().request_screenshot(move |image| match enregistrer_png(&image, &chemin) {
                    Ok(()) => println!("capture : {}", chemin.display()),
                    Err(e) => eprintln!("capture {} : {e}", chemin.display()),
                });
            }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.mesure {
            println!("phase            images   ambiance (médiane, p90)   image entière (médiane, p90)");
            let p90 = |v: &[f64]| {
                let mut v = v.to_vec();
                v.sort_by(f64::total_cmp);
                v.get(v.len() * 9 / 10).copied().unwrap_or(0.0)
            };
            for (phase, cout, image) in &self.releves {
                println!(
                    "{phase:<16} {:>6}   {:>8.3} ms {:>8.3} ms     {:>8.3} ms {:>8.3} ms",
                    cout.len(),
                    mediane(cout).unwrap_or(0.0),
                    p90(cout),
                    mediane(image).unwrap_or(0.0),
                    p90(image)
                );
            }
        }
    }
}

// ---- L'historique des conversations (le lot 6.3) ------------------------------------------------------------------

/// La personne de l'essai : jamais celle d'une vraie Eve.
const QUI_D_ESSAI: &str = "Essai";

/// `eve --essai-historique [dossier] [--captures DOSSIER]` : le panneau de l'historique dans une fenêtre à lui, posé
/// comme la fenêtre native le posera — dans une `Area` au premier plan, par-dessus la conversation, Échap et le clic
/// dehors compris —, sur des conversations inventées que l'essai écrit dans `dossier` (d'office
/// `%TEMP%\eve-essai-historique`) : `historique\Essai\essai.jsonl`, un seul fichier réécrit à chaque lancement (rien de
/// vieux n'y reste, rien ne s'efface), et le plan d'une carte d'aperçu. Il ne lit que ce dossier, jamais l'historique
/// d'une vraie Eve. Le bouton « Historique » de la barre ouvre et ferme le panneau ; ce qu'il rend (reprendre, le geste
/// d'une carte) s'affiche dans la barre et à la console. `--captures` joue un parcours — la liste, le survol, la
/// recherche, rien qui corresponde, une conversation et sa suite, le retour, ce qui a mal fini, Échap, la réouverture,
/// une personne sans historique, les cartes, la reprise, une fenêtre étroite, le clic dehors, le clavier — avec une
/// capture PNG à chaque étape dans DOSSIER, puis ferme la fenêtre ; ses clics, ses frappes et sa molette sont des
/// événements d'egui donnés à cette fenêtre seule.
pub fn historique(args: &[String]) -> i32 {
    let captures =
        args.iter().position(|a| a == "--captures").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    let dossier = args
        .iter()
        .enumerate()
        .find(|(k, a)| !a.starts_with("--") && (*k == 0 || args[k - 1] != "--captures"))
        .map_or_else(|| std::env::temp_dir().join("eve-essai-historique"), |(_, a)| std::path::PathBuf::from(a));
    let aujourdhui = crate::journal::date();
    let entrees = match ecrire_l_historique_d_essai(&dossier, &aujourdhui) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("{} : {e}", dossier.display());
            return 1;
        }
    };
    if let Some(c) = &captures
        && let Err(e) = std::fs::create_dir_all(c)
    {
        eprintln!("{} : {e}", c.display());
        return 1;
    }
    let parcours = if captures.is_some() { parcours_de_l_historique(&entrees) } else { Vec::new() };
    let titre = "Eve — Essai de l'historique";
    // Le parcours ne prend pas le focus à ce qu'on fait : ses gestes ne passent pas par Windows.
    let vue = egui::ViewportBuilder::default()
        .with_title(titre)
        .with_inner_size([1180.0, 840.0])
        .with_min_inner_size([420.0, 500.0])
        .with_active(captures.is_none());
    let options = eframe::NativeOptions { viewport: vue, ..Default::default() };
    let local = crate::reglages::dossier_local();
    let creer: eframe::AppCreator<'_> = Box::new(move |cc| {
        crate::natif::polices(&cc.egui_ctx);
        crate::natif::apparence(&cc.egui_ctx);
        Ok(Box::new(EssaiDeLHistorique {
            panneau: Some(crate::historique_ui::Historique::nouveau().sous(dossier.clone())),
            cartes: crate::cartes::Cartes::nouvelles(Some(dossier.clone()), local),
            dossier,
            qui: QUI_D_ESSAI.to_string(),
            rendu: String::new(),
            bouton: egui::Rect::NOTHING,
            place: egui::Rect::NOTHING,
            parcours,
            suivant: 0,
            debut: Instant::now(),
            decalage: 0.0,
            attente: None,
            pointeur: egui::Pos2::ZERO,
            captures,
            a_capturer: Vec::new(),
        }))
    });
    match eframe::run_native(titre, options, creer) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("la fenêtre ne s'ouvre pas : {e}");
            1
        }
    }
}

/// Le plan de la carte d'aperçu d'une conversation inventée, en SVG (que les cartes dessinent avec son texte).
const PLAN_D_ESSAI: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 720 440" width="720" height="440">
  <rect width="720" height="440" fill="#FFFFFF"/>
  <g fill="none" stroke="#18212B" stroke-width="6" stroke-linejoin="round">
    <rect x="40" y="40" width="640" height="360"/>
    <path d="M360 40V250M40 250H250M310 250H520V400"/>
  </g>
  <g fill="none" stroke="#8A97A2" stroke-width="1.5" stroke-dasharray="6 5">
    <path d="M250 250A60 60 0 0 1 310 310"/>
  </g>
  <g font-family="Segoe UI, sans-serif" font-size="21" fill="#4B5765" text-anchor="middle">
    <text x="200" y="150">Salle familiale</text>
    <text x="520" y="150">Chambre d'amis</text>
    <text x="280" y="335">Rangement</text>
    <text x="600" y="335">Salle mécanique</text>
  </g>
  <text x="680" y="428" font-family="Segoe UI, sans-serif" font-size="12" fill="#8A97A2"
        text-anchor="end">Plan d'exemple · 1:50</text>
</svg>
"##;

/// Le jour `recul` jours avant `aujourdhui` (« AAAA-MM-JJ »), par le calendrier civil de Howard Hinnant, dans les deux
/// sens (comme `journal.rs`).
fn jour_d_essai(aujourdhui: &str, recul: i64) -> String {
    let p: Vec<i64> = aujourdhui.split('-').filter_map(|x| x.parse().ok()).collect();
    let [a, m, j] = p[..] else { return aujourdhui.to_string() };
    let a = if m <= 2 { a - 1 } else { a };
    let ere = a.div_euclid(400);
    let yoe = a - ere * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + j - 1;
    let z = ere * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - recul;
    let ere = z.div_euclid(146_097);
    let doe = z - ere * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (j, m) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{m:02}-{j:02}", yoe + ere * 400 + i64::from(m <= 2))
}

/// L'historique inventé de l'essai, conversation par conversation, daté depuis aujourd'hui.
struct HistoriqueInvente<'a> {
    aujourdhui: &'a str,
    /// La conversation en cours d'écriture : l'heure de son début (« AAAA-MM-JJ HH-MM-00 »).
    id: String,
    entrees: Vec<crate::historique::Entree>,
}

impl HistoriqueInvente<'_> {
    /// Une conversation commence, `recul` jours avant aujourd'hui, à l'heure `debut`.
    fn conversation(&mut self, recul: i64, debut: &str) {
        self.id = format!("{} {}-00", jour_d_essai(self.aujourdhui, recul), debut.replace(':', "-"));
    }

    /// Un échange de la conversation en cours ; ses pièces et ce qui a mal fini se posent sur ce qu'il rend.
    fn echange(&mut self, recul: i64, heure: &str, demande: &str, reponse: &str) -> &mut crate::historique::Entree {
        self.entrees.push(crate::historique::Entree {
            conversation: self.id.clone(),
            date: jour_d_essai(self.aujourdhui, recul),
            heure: heure.to_string(),
            demande: demande.to_string(),
            reponse: reponse.to_string(),
            pieces: Vec::new(),
            issue: String::new(),
        });
        self.entrees.last_mut().expect("l'échange vient de s'ajouter")
    }
}

/// Les conversations inventées de l'essai, datées depuis `aujourdhui` (aujourd'hui, hier, la semaine, le mois, l'an
/// dernier), dans l'ordre où elles se sont dites : une reprise un autre jour, des pièces jointes, une réponse
/// interrompue, une autre perdue, une demande sans mots, des cartes (un agenda, un tableau, un brouillon, l'aperçu du
/// plan), un tableau en markdown, un bloc de code. Aucune personne de vrai.
fn conversations_d_essai(dossier: &Path, aujourdhui: &str) -> Vec<crate::historique::Entree> {
    use serde_json::json;
    let carte = |v: serde_json::Value| format!("```carte\n{v}\n```");
    let agenda = carte(json!({ "type": "agenda", "titre": "Aujourd'hui", "evenements": [
        { "heure": "8 h 30", "titre": "Appeler l'inspecteur municipal", "sorte": "appel" },
        { "heure": "13 h", "titre": "Visite du chantier des Érables", "detail": "12, rue des Érables",
          "sorte": "confirme" },
        { "heure": "16 h 30", "titre": "Remettre la soumission du garage", "detail": "Échéance du client",
          "sorte": "echeance" }
    ] }));
    let factures = carte(json!({
        "type": "tableau",
        "titre": "Factures de la semaine",
        "colonnes": ["Fournisseur", "Nº", "Échéance", "Montant"],
        "lignes": [
            ["Plomberie Exemple", "96048", "jeudi", "1\u{a0}204,50 $"],
            ["Électricité du Nord", "E-2231", "vendredi", "1\u{a0}183,38 $"],
            ["Matériaux Exemple", "9013623395", "vendredi", "31,08 $"]
        ],
        "pied": ["Total", "", "", "2\u{a0}418,96 $"]
    }));
    let brouillon = carte(json!({
        "type": "brouillon",
        "a": "Julie (Plomberie Exemple)",
        "objet": "Facture 96048 : le paiement part jeudi",
        "extrait": "Bonjour Julie,\nLa facture 96048 sera payée jeudi, par virement.\nMerci de ta patience,\n\
                    Bonne journée !",
        "id": "00000000A1B2C3D4E5F60718293A4B5C"
    }));
    let plan = dossier.join("plan-du-sous-sol.svg").display().to_string();
    let apercu = carte(json!({ "type": "apercu", "titre": "Plan du sous-sol", "chemin": plan }));
    let mut h = HistoriqueInvente { aujourdhui, id: String::new(), entrees: Vec::new() };

    h.conversation(0, "09:13");
    let reponse = format!("Trois choses aujourd'hui, dont la visite du chantier à 13 h.\n---\n{agenda}");
    h.echange(0, "09:14", "Qu'est-ce que j'ai à l'agenda aujourd'hui ?", &reponse);
    let reponse = "Demain, rien avant 10 h 30 : l'inspection de la dalle, puis l'appel avec Julie à 15 h.";
    h.echange(0, "09:16", "Et demain ?", reponse);
    h.echange(0, "09:21", "Rappelle-moi d'appeler Plomberie Exemple à 15 h.", "C'est noté : je te le rappelle à 15 h.");

    h.conversation(0, "07:41");
    let coupee = "Tu as quatre courriels non lus. Le premier vient de Matériaux Exemple : la livraison du bois est \
                  devancée à jeudi, et";
    h.echange(0, "07:42", "Lis-moi les courriels non lus.", coupee).issue = "interrompue".into();

    h.conversation(1, "16:04");
    let reponse = format!(
        "Trois factures arrivent à échéance cette semaine, pour **2\u{a0}418,96 $** en tout.\n---\n{factures}\nLa \
         plus pressée est celle de Plomberie Exemple, due jeudi."
    );
    h.echange(1, "16:05", "Combien me reste-t-il à payer cette semaine ?", &reponse);
    let reponse = format!("Le brouillon est prêt dans Outlook : relis-le avant de l'envoyer.\n---\n{brouillon}");
    h.echange(1, "16:12", "Prépare un courriel à Julie pour la facture 96048.", &reponse);
    let reponse = format!("Voici le plan du sous-sol, tel que l'architecte l'a envoyé.\n---\n{apercu}");
    h.echange(1, "16:31", "Montre-moi le plan du sous-sol.", &reponse);
    h.echange(1, "16:40", "Merci !", "Avec plaisir.");

    // Commencée il y a trois jours, reprise hier : elle se range à hier.
    h.conversation(3, "10:01");
    let reponse = "Voici de quoi faire cinq soupers :\n- Poulet, riz et brocoli\n- Pâtes au saumon\n- Tacos au bœuf\n\
                   - Soupe aux lentilles\n- Pizza maison\n---\nJe l'ai rangée dans `epicerie.md`.";
    h.echange(3, "10:02", "Fais-moi une liste d'épicerie pour la semaine.", reponse);
    h.echange(1, "08:15", "Ajoute du lait et des pommes.", "C'est ajouté : du lait (2 L) et des pommes Cortland.");

    h.conversation(2, "18:29");
    let reponse = "Une lecture, le chapitre 4, et la page 52 du cahier de mathématiques.";
    h.echange(2, "18:30", "Quels devoirs Léa a-t-elle pour demain ?", reponse);
    let reponse = "La sortie au musée est vendredi ; l'autorisation est à signer d'ici jeudi.";
    h.echange(2, "18:33", "Et la sortie scolaire ?", reponse);

    h.conversation(5, "12:09");
    let reponse = "Pas de neige prévue : samedi nuageux, 4 °C ; dimanche ensoleillé, 7 °C.";
    h.echange(5, "12:10", "Va-t-il neiger cette fin de semaine ?", reponse);

    h.conversation(6, "08:59");
    let reponse = "642 km, dont 418 pour le chantier des Érables.";
    h.echange(6, "09:00", "Combien de kilomètres ai-je faits ce mois-ci ?", reponse);

    h.conversation(9, "13:29");
    let reponse = "Je l'ai classée dans **Factures › 2026 › Matériaux Exemple**, au 005, section 12 : 31,08 $ avec les \
                   taxes.";
    h.echange(9, "13:30", "Classe cette facture.", reponse).pieces =
        vec!["Facture Matériaux Exemple 9013623395.pdf".into()];
    let reponse = "La soumission 2026-114 est rangée avec le projet du garage, la photo dans `Chantier/Photos`.\n---\n\
                   La soumission expire le 15 novembre.";
    h.echange(9, "13:34", "Et la soumission, avec la photo ?", reponse).pieces =
        vec!["Soumission 2026-114.pdf".into(), "Photo du chantier.jpg".into()];

    h.conversation(12, "20:04");
    let reponse = "Sur 1\u{a0}250,00 $ : la TPS (5 %) fait 62,50 $ et la TVQ (9,975 %) 124,69 $, pour \
                   **1\u{a0}437,19 $** en tout.\n---\nLe calcul, si tu veux le refaire :\n```python\nmontant = \
                   1250.00\ntps = round(montant * 0.05, 2)\ntvq = round(montant * 0.09975, 2)\nprint(montant + tps + \
                   tvq)\n```";
    h.echange(12, "20:05", "Comment je calcule la TPS et la TVQ sur 1 250 $ ?", reponse);

    // Une réponse perdue : le tour s'est mal terminé.
    h.conversation(24, "19:09");
    h.echange(24, "19:10", "Prépare le budget du mois.", "").issue = "erreur".into();

    // Une numérisation envoyée sans un mot.
    h.conversation(24, "06:59");
    let reponse = "J'ai bien reçu la numérisation : trois pages, rangées dans `Numérisations`.";
    h.echange(24, "07:00", "", reponse).pieces = vec!["Numérisation Eve.pdf".into()];

    h.conversation(40, "07:59");
    let reponse = "| Entrepreneur | Prix | Garantie |\n|---|---|---|\n| Toitures Exemple | 14\u{a0}800 $ | 10 ans |\n\
                   | Couvreur du Nord | 13\u{a0}950 $ | 7 ans |\n\nToitures Exemple coûte 850 $ de plus, mais garantit \
                   trois ans de plus.";
    h.echange(40, "08:00", "Compare les deux soumissions pour la toiture.", reponse);
    let reponse = "Toitures Exemple : sur une toiture de vingt ans, trois ans de garantie de plus valent bien 850 $.";
    h.echange(40, "08:04", "Laquelle me conseilles-tu ?", reponse);

    // L'an dernier : son jour dit son année.
    h.conversation(310, "20:14");
    let reponse = "Le jeudi 18 décembre, à 18 h 30, au gymnase.";
    h.echange(310, "20:15", "Quand est le spectacle de Noël de l'école ?", reponse);

    // Dans l'ordre où elles se sont dites, comme Eve les ajoute au bout de son fichier.
    let mut entrees = h.entrees;
    entrees.sort_by(|a, b| (&a.date, &a.heure).cmp(&(&b.date, &b.heure)));
    entrees
}

/// Écrit l'historique inventé dans `dossier` (`historique\Essai\essai.jsonl`, réécrit en entier) et le plan de sa
/// carte d'aperçu ; rend ses échanges.
fn ecrire_l_historique_d_essai(dossier: &Path, aujourdhui: &str) -> std::io::Result<Vec<crate::historique::Entree>> {
    let entrees = conversations_d_essai(dossier, aujourdhui);
    let personne = crate::historique::dossier_dans(dossier, QUI_D_ESSAI);
    std::fs::create_dir_all(&personne)?;
    std::fs::write(dossier.join("plan-du-sous-sol.svg"), PLAN_D_ESSAI)?;
    let mut lignes = String::new();
    for e in &entrees {
        lignes.push_str(&serde_json::to_string(e).map_err(std::io::Error::other)?);
        lignes.push('\n');
    }
    std::fs::write(personne.join("essai.jsonl"), lignes)?;
    Ok(entrees)
}

/// Ce qu'un pas du parcours vise : un élément du panneau (sa place à l'image d'avant), la place du panneau, le bouton
/// de la barre, un point de la fenêtre.
#[derive(Clone, Debug)]
enum Cible {
    Effacer,
    Reprendre,
    Conversation(String),
    Place,
    BoutonHistorique,
    Point(egui::Pos2),
}

/// Un pas du parcours de `--captures`.
#[derive(Clone, Debug)]
enum PasDEssai {
    Survoler(Cible),
    Appuyer,
    Relacher,
    Taper(&'static str),
    Touche(egui::Key),
    Molette(f32),
    Capture(&'static str),
    Qui(&'static str),
    Taille(f32, f32),
    Fin,
}

/// Le parcours de `--captures` : chaque pas à son heure (en secondes depuis l'ouverture), le temps qu'egui dessine
/// ce qu'il a changé avant chaque capture (le fondu, la recherche relancée 220 ms après la frappe, une lecture).
fn parcours_de_l_historique(entrees: &[crate::historique::Entree]) -> Vec<(f64, PasDEssai)> {
    use PasDEssai::{Appuyer, Capture, Fin, Molette, Qui, Relacher, Survoler, Taille, Taper, Touche};
    // L'id d'une conversation, par le début de sa première demande.
    let id = |debut: &str| {
        let e = entrees.iter().find(|e| e.demande.starts_with(debut)).expect("une conversation de l'essai");
        Cible::Conversation(e.conversation.clone())
    };
    let (agenda, courriels, factures, budget) =
        (id("Qu'est-ce que j'ai"), id("Lis-moi"), id("Combien me reste"), id("Prépare le budget"));
    let mut v: Vec<(f64, PasDEssai)> = Vec::new();
    let mut cliquer = |t: f64, cible: Cible| {
        v.extend([(t, Survoler(cible)), (t + 0.1, Appuyer), (t + 0.2, Relacher)]);
    };
    cliquer(3.8, Cible::Effacer);
    cliquer(4.6, factures.clone());
    cliquer(8.0, courriels);
    cliquer(9.8, budget);
    cliquer(11.6, Cible::BoutonHistorique);
    cliquer(13.4, agenda);
    cliquer(14.8, Cible::Reprendre);
    cliquer(16.2, Cible::BoutonHistorique);
    cliquer(17.2, factures.clone());
    cliquer(18.4, Cible::Point(egui::pos2(24.0, 26.0)));
    cliquer(19.2, Cible::BoutonHistorique);
    v.extend([
        (0.9, Capture("01-liste")),
        (1.1, Survoler(factures)),
        (1.5, Capture("02-survol")),
        (1.7, Taper("fact")),
        (2.6, Capture("03-recherche")),
        (2.8, Taper("urex")),
        (3.6, Capture("04-rien-ne-correspond")),
        (5.4, Capture("05-conversation")),
        (5.5, Survoler(Cible::Place)),
        (5.6, Molette(-560.0)),
        (6.4, Capture("06-conversation-suite")),
        (6.6, Survoler(Cible::Reprendre)),
        (7.0, Capture("07-reprendre-survol")),
        (7.2, Touche(egui::Key::Escape)),
        (7.8, Capture("08-retour-a-la-liste")),
        (8.8, Capture("09-interrompue")),
        (9.0, Touche(egui::Key::Escape)),
        (9.2, Taper("budget")),
        (10.6, Capture("10-sans-reponse")),
        (10.8, Touche(egui::Key::Escape)),
        (11.0, Touche(egui::Key::Escape)),
        (11.4, Capture("11-ferme")),
        (12.4, Capture("12-rouvert")),
        (12.6, Qui("Personne")),
        (13.2, Capture("13-personne-sans-historique")),
        (13.3, Qui(QUI_D_ESSAI)),
        (14.4, Capture("14-cartes")),
        (15.4, Capture("15-reprise")),
        (15.6, Taille(520.0, 860.0)),
        (17.0, Capture("16-etroite-liste")),
        (18.2, Capture("17-etroite-conversation")),
        (19.0, Capture("18-clic-dehors")),
        // Au clavier : de la recherche à la deuxième conversation, puis Entrée l'ouvre.
        (19.9, Touche(egui::Key::Tab)),
        (20.1, Touche(egui::Key::Tab)),
        (20.4, Capture("19-clavier")),
        (20.6, Touche(egui::Key::Enter)),
        (21.4, Capture("20-clavier-entree")),
        (21.6, Fin),
    ]);
    // Un tri stable : deux pas à la même heure gardent leur ordre.
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v
}

struct EssaiDeLHistorique {
    /// Le panneau, quand il est ouvert ; le dossier de l'essai, d'où il lit.
    panneau: Option<crate::historique_ui::Historique>,
    dossier: std::path::PathBuf,
    cartes: crate::cartes::Cartes,
    qui: String,
    /// Ce que le panneau a rendu en dernier, dans la barre.
    rendu: String,
    /// Le bouton « Historique » de la barre et la place du panneau, à la dernière image.
    bouton: egui::Rect,
    place: egui::Rect,
    /// Le parcours de `--captures`, son prochain pas, son départ ; le temps qu'il a passé à attendre ses cibles, et
    /// depuis quand il attend celle du pas en cours ; le pointeur qu'il promène ; les captures à faire.
    parcours: Vec<(f64, PasDEssai)>,
    suivant: usize,
    debut: Instant,
    decalage: f64,
    attente: Option<f64>,
    pointeur: egui::Pos2,
    captures: Option<std::path::PathBuf>,
    a_capturer: Vec<&'static str>,
}

impl EssaiDeLHistorique {
    fn position(&self, ctx: &egui::Context, cible: &Cible) -> Option<egui::Pos2> {
        use crate::historique_ui::{Element, id_de};
        let element = |e: Element<'_>| ctx.read_response(id_de(e)).map(|r| r.rect.center());
        match cible {
            Cible::Effacer => element(Element::Effacer),
            Cible::Reprendre => element(Element::Reprendre),
            Cible::Conversation(id) => element(Element::Conversation(id)),
            Cible::Place => Some(self.place.center()),
            Cible::BoutonHistorique => Some(self.bouton.center()),
            Cible::Point(p) => Some(*p),
        }
    }

    /// Le bouton de la barre : il ouvre le panneau, ou le ferme.
    fn basculer(&mut self) {
        self.panneau = match self.panneau {
            Some(_) => None,
            None => Some(crate::historique_ui::Historique::nouveau().sous(self.dossier.clone())),
        };
    }
}

impl eframe::App for EssaiDeLHistorique {
    /// Les pas du parcours venus à leur heure, en événements d'egui.
    fn raw_input_hook(&mut self, ctx: &egui::Context, entree: &mut egui::RawInput) {
        if self.parcours.is_empty() {
            return;
        }
        // Le parcours ne prend que ses propres gestes : la vraie souris qui passerait sur la fenêtre (Windows lui
        // envoie ses mouvements, même inactive) déplacerait le pointeur entre un appui et son relâchement.
        entree.events.retain(|e| {
            !matches!(
                e,
                egui::Event::PointerMoved(_)
                    | egui::Event::MouseMoved(_)
                    | egui::Event::PointerButton { .. }
                    | egui::Event::PointerGone
                    | egui::Event::MouseWheel { .. }
                    | egui::Event::Key { .. }
                    | egui::Event::Text(_)
            )
        });
        entree.focused = true;
        let maintenant = self.debut.elapsed().as_secs_f64();
        // L'heure du parcours : celle de l'horloge, moins ce qu'il a attendu ses cibles.
        while self.suivant < self.parcours.len() && self.parcours[self.suivant].0 <= maintenant - self.decalage {
            let pas = self.parcours[self.suivant].1.clone();
            // Une cible pas encore dessinée (une liste qui se lit encore) : le parcours l'attend, et sa suite avec lui ;
            // deux secondes au plus, puis il saute le geste entier, sans appuyer à l'ancienne place du pointeur.
            if let PasDEssai::Survoler(cible) = &pas
                && self.position(ctx, cible).is_none()
            {
                let depuis = *self.attente.get_or_insert(maintenant);
                if maintenant - depuis < 2.0 {
                    break;
                }
                eprintln!("essai : {cible:?} introuvable");
                self.attente = None;
                self.suivant += 1;
                while matches!(self.parcours.get(self.suivant), Some((_, PasDEssai::Appuyer | PasDEssai::Relacher))) {
                    self.suivant += 1;
                }
                continue;
            }
            if let Some(depuis) = self.attente.take() {
                self.decalage += maintenant - depuis;
            }
            self.suivant += 1;
            let rien = egui::Modifiers::NONE;
            match pas {
                PasDEssai::Survoler(cible) => {
                    if let Some(p) = self.position(ctx, &cible) {
                        self.pointeur = p;
                        entree.events.push(egui::Event::PointerMoved(p));
                    }
                }
                PasDEssai::Appuyer | PasDEssai::Relacher => entree.events.push(egui::Event::PointerButton {
                    pos: self.pointeur,
                    button: egui::PointerButton::Primary,
                    pressed: matches!(pas, PasDEssai::Appuyer),
                    modifiers: rien,
                }),
                PasDEssai::Taper(texte) => entree.events.push(egui::Event::Text(texte.to_string())),
                PasDEssai::Touche(key) => {
                    for pressed in [true, false] {
                        entree.events.push(egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: rien,
                        });
                    }
                }
                PasDEssai::Molette(dy) => entree.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, dy),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: rien,
                }),
                PasDEssai::Capture(nom) => self.a_capturer.push(nom),
                PasDEssai::Qui(qui) => self.qui = qui.to_string(),
                PasDEssai::Taille(l, h) => ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(l, h))),
                PasDEssai::Fin => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        use eframe::egui::{Align, Layout, Margin, RichText, UiBuilder, pos2, vec2};

        use crate::historique_ui::{Sortie, icones};
        use crate::natif::{self, Bouton, Ligne};
        let ctx = ui.ctx().clone();
        egui::Panel::top("essai-historique-barre")
            .exact_size(52.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(couleurs::BARRE_HAUT).inner_margin(Margin::symmetric(16, 0)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(natif::semi("Essai de l'historique", 15.0).color(egui::Color32::WHITE));
                    ui.label(RichText::new(format!("·  {}", self.qui)).size(12.0).color(couleurs::BARRE_TEXTE));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let ouvert = self.panneau.is_some();
                        let r = natif::bouton(ui, Some(icones::HISTORIQUE), "Historique", Bouton::Barre(ouvert));
                        self.bouton = r.rect;
                        if r.on_hover_text("Historique des conversations").clicked() {
                            self.basculer();
                        }
                        ui.add_space(10.0);
                        let rendu = if self.rendu.is_empty() { "Aucun geste" } else { self.rendu.as_str() };
                        ui.add(
                            egui::Label::new(RichText::new(rendu).size(12.0).color(couleurs::BARRE_CLAIR)).truncate(),
                        );
                    });
                });
            });
        // La conversation, dessous : ce que le panneau recouvre, et une saisie pour la ressemblance.
        egui::CentralPanel::default().frame(egui::Frame::new().fill(couleurs::SURFACE)).show(ui, |ui| {
            let r = ui.max_rect();
            let colonne = egui::Rect::from_center_size(r.center(), vec2(r.width().min(800.0), r.height()));
            let colonne = colonne.shrink2(vec2(16.0, 0.0));
            let saisie = egui::Rect::from_min_max(pos2(colonne.left(), colonne.bottom() - 62.0), colonne.max);
            // La place du panneau, celle de la page : le corps de la conversation, de la barre à la saisie, à 10
            // points de ses bords (`inset: 10px` sur `main`) ; les bulles en sont à 16 points : il les couvre.
            let corps =
                egui::Rect::from_min_max(colonne.min, pos2(colonne.right(), saisie.top())).expand2(vec2(16.0, 0.0));
            self.place = corps.shrink(10.0);
            let cartes = &mut self.cartes;
            ui.scope_builder(UiBuilder::new().max_rect(colonne), |ui| {
                ui.add_space(22.0);
                ui.vertical_centered(|ui| {
                    ui.label(natif::semi("Prête", 17.0).color(couleurs::ENCRE));
                    let aide = "Dis « Eve… » — ou maintiens [Ctrl droit] pour parler";
                    ui.label(RichText::new(aide).size(12.0).color(couleurs::ENCRE_3));
                });
                ui.add_space(18.0);
                let mut gestes = Vec::new();
                for (de, texte, heure) in [
                    ("toi", "Qu'est-ce que j'ai demain ?", "09:30"),
                    ("eve", "Demain, l'inspection de la dalle à 10 h 30, puis l'appel avec Julie à 15 h.", "09:30"),
                    ("toi", "Merci !", "09:31"),
                    ("eve", "Avec plaisir.", "09:31"),
                ] {
                    let ligne = Ligne { de, texte, heure, ancien: false, pieces: &[], en_cours: false, tour: "" };
                    natif::bulle(ui, &ligne, cartes, &mut gestes);
                }
            });
            let champ = saisie.shrink2(vec2(0.0, 10.0));
            let p = ui.painter();
            p.rect(champ, 14.0, couleurs::FOND, egui::Stroke::new(1.0, couleurs::FILET_FORT), egui::StrokeKind::Inside);
            let indice = egui::FontId::proportional(14.5);
            let debut = pos2(champ.left() + 18.0, champ.center().y);
            p.text(debut, egui::Align2::LEFT_CENTER, "Écris ta demande à Eve…", indice, couleurs::ENCRE_3);
        });
        // Le panneau, par-dessus, posé comme la fenêtre native le posera.
        if let Some(h) = self.panneau.as_mut() {
            let (place, bouton) = (self.place, self.bouton);
            let (qui, cartes) = (self.qui.as_str(), &mut self.cartes);
            let sortie = egui::Area::new(egui::Id::unique("essai-historique-panneau"))
                .order(egui::Order::Foreground)
                .fixed_pos(place.min)
                .constrain(false)
                .fade_in(false)
                .show(&ctx, |ui| {
                    ui.scope_builder(UiBuilder::new().max_rect(place), |ui| h.dessiner(ui, qui, cartes)).inner
                })
                .inner;
            // Échap : de la conversation à la liste, puis le panneau se ferme ; un clic hors du panneau et de son
            // bouton le ferme (et va aussi à ce qu'il touche, comme dans la page).
            let echap = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && h.echap();
            let dehors = ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer.press_origin().is_some_and(|p| !place.contains(p) && !bouton.contains(p))
            });
            let mut fermer = echap || dehors;
            match sortie {
                Some(Sortie::Fermer) => fermer = true,
                Some(Sortie::Reprendre(id)) => {
                    self.rendu = format!("Geste : reprendre — {id}");
                    println!("{}", self.rendu);
                    fermer = true;
                }
                Some(Sortie::Geste(g)) => {
                    let valeur = g.valeur.as_str().map_or(g.valeur.to_string(), str::to_string);
                    self.rendu = format!("Geste : {} — {valeur}", g.action);
                    println!("{}", self.rendu);
                }
                None => {}
            }
            if fermer {
                self.panneau = None;
            }
        }
        if let Some(dossier) = &self.captures {
            for nom in self.a_capturer.drain(..) {
                let chemin = dossier.join(format!("historique-{nom}.png"));
                ctx.request_screenshot(move |image| match enregistrer_png(&image, &chemin) {
                    Ok(()) => println!("capture : {}", chemin.display()),
                    Err(e) => eprintln!("capture {} : {e}", chemin.display()),
                });
            }
        }
        let rythme = if self.parcours.is_empty() { 250 } else { 16 };
        ctx.request_repaint_after(Duration::from_millis(rythme));
    }
}

#[cfg(test)]
mod essais_de_l_historique {
    use super::*;

    #[test]
    fn les_jours_de_l_essai_se_comptent_a_rebours() {
        assert_eq!(jour_d_essai("2026-10-24", 0), "2026-10-24");
        assert_eq!(jour_d_essai("2026-10-24", 1), "2026-10-23");
        assert_eq!(jour_d_essai("2026-03-01", 1), "2026-02-28");
        assert_eq!(jour_d_essai("2028-03-01", 1), "2028-02-29", "2028 est bissextile");
        assert_eq!(jour_d_essai("2026-01-01", 1), "2025-12-31");
        assert_eq!(jour_d_essai("2026-10-24", 310), "2025-12-18");
    }

    #[test]
    fn l_historique_de_l_essai_se_relit() {
        let dossier = std::env::temp_dir().join(format!("eve-essai-historique-{}", std::process::id()));
        let entrees = ecrire_l_historique_d_essai(&dossier, "2026-10-24").expect("l'historique s'écrit");
        let toutes = crate::historique::conversations_dans(&dossier, QUI_D_ESSAI, "", 300);
        let factures = crate::historique::conversations_dans(&dossier, QUI_D_ESSAI, "fact", 300);
        let aucune = crate::historique::conversations_dans(&dossier, "Personne", "", 300);
        let reprise = toutes.iter().find(|c| c.titre.starts_with("Fais-moi")).map(|c| c.fin_date.clone());
        let _ = std::fs::remove_dir_all(&dossier);
        assert_eq!(toutes.len(), 13);
        assert_eq!(toutes.iter().map(|c| c.echanges).sum::<usize>(), entrees.len());
        assert_eq!(toutes[0].titre, "Qu'est-ce que j'ai à l'agenda aujourd'hui ?", "la plus récente d'abord");
        assert_eq!(reprise.as_deref(), Some("2026-10-23"), "l'épicerie, reprise hier");
        assert_eq!(factures.len(), 2);
        assert!(aucune.is_empty());
        // Le parcours trouve chacune de ses conversations.
        assert!(!parcours_de_l_historique(&entrees).is_empty());
    }
}

// ---- Constructo AI : un faux serveur, pour essayer la connexion d'Eve -------------------------------------------------

/// `eve --essai-constructo [port] [--duree secondes] [--outils N] [--reveil secondes] [--panne outil]… [--rediriger]` :
/// le faux Constructo AI ([`FauxConstructo`]), sur le poste, jusqu'à Ctrl+C (le port 47997 d'office ; des jetons
/// d'accès d'une heure, d'une minute au moins). Une Eve d'essai s'y relie avec
/// `EVE_CONSTRUCTO_MCP=http://127.0.0.1:<port>`, et `EVE_SANS_NAVIGATEUR=1` : elle joue alors elle-même la page de
/// connexion, sans ouvrir de navigateur ; son tableau de bord y lit une entreprise inventée. `--outils N` : N lectures
/// de plus, pour mesurer ce que coûtent des centaines d'outils ; `--reveil S` : son connecteur MCP dort, comme le vrai
/// sur Render — la première requête le réveille, et toutes attendent S secondes (mesuré le 24 octobre 2026 : avec
/// 40 s, Claude Code attend, `MCP_TIMEOUT` aidant) ; `--panne outil` : cet outil rend le texte d'erreur du vrai ;
/// `--rediriger` : « /mcp » renvoie vers « /mcp/ », comme le `Mount` de Starlette. Chaque requête s'imprime — jamais
/// un jeton.
pub fn constructo(args: &[String]) -> i32 {
    let nombre = |nom: &str| {
        args.iter().position(|a| a == nom).and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<u64>().ok())
    };
    let port = args.first().and_then(|a| a.parse::<u16>().ok()).unwrap_or(47_997);
    let duree = nombre("--duree").unwrap_or(3600).max(60);
    let nombreux = nombre("--outils").unwrap_or(0).min(2000) as usize;
    let faux = match FauxConstructo::demarrer(port, duree, nombreux) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("le faux Constructo AI ne s'ouvre pas sur le port {port} : {e}");
            return 1;
        }
    };
    if let Some(s) = nombre("--reveil") {
        faux.endormir(s.min(300));
    }
    for (i, a) in args.iter().enumerate() {
        if a == "--panne"
            && let Some(outil) = args.get(i + 1)
        {
            faux.faire_echouer(outil);
        }
    }
    if args.iter().any(|a| a == "--rediriger") {
        faux.rediriger();
    }
    println!(
        "faux Constructo AI : http://127.0.0.1:{} — des jetons de {duree} s, {} outils ; Ctrl+C l'arrête.\n\
         Une Eve d'essai s'y relie avec EVE_CONSTRUCTO_MCP=http://127.0.0.1:{} et EVE_SANS_NAVIGATEUR=1.",
        faux.port,
        faux_constructo::OUTILS.len() + nombreux,
        faux.port
    );
    let mut vus = 0;
    loop {
        let journal = faux.journal();
        for ligne in &journal[vus..] {
            println!("{} {ligne}", crate::journal::heure());
        }
        vus = journal.len();
        thread::sleep(Duration::from_millis(200));
    }
}

#[cfg(test)]
pub use faux_constructo::demander_sans_suivre;
pub use faux_constructo::{FauxConstructo, jouer_le_navigateur};

mod faux_constructo {
    use std::collections::{HashMap, HashSet};
    use std::io::{self, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use crate::centre::{date_du_numero, numero_du_jour};
    use crate::connexion::{b64url, defi, maintenant};
    use crate::ecran::{decoder, jeton_aleatoire};
    use crate::https::encoder;

    /// Un outil du faux serveur : son nom, sa description, ses paramètres (et leur type), ceux qu'il exige.
    type Outil = (&'static str, &'static str, &'static [(&'static str, &'static str)], &'static [&'static str]);

    /// Les outils du faux serveur : les lectures du tableau de bord d'Eve (`entreprise.rs`), deux autres lectures, et
    /// trois écritures qu'Eve doit faire confirmer avant qu'elles n'arrivent ici.
    pub const OUTILS: [Outil; 18] = [
        ("tableau_kpis", "KPIs consolidés du tableau de bord. Lecture seule.", &[("periode_jours", "integer")], &[]),
        ("tableau_alertes", "Alertes opérationnelles consolidées. Lecture seule.", &[], &[]),
        (
            "tableau_factures_agees",
            "Balance âgée des comptes clients. Lecture seule.",
            &[("inclure_detail", "boolean"), ("limite", "integer")],
            &[],
        ),
        ("tableau_pipeline_ventes", "Pipeline commercial par statut. Lecture seule.", &[], &[]),
        (
            "lister_projets",
            "Liste les projets de construction.",
            &[("statut", "string"), ("client_id", "integer"), ("recherche", "string"), ("limite", "integer")],
            &[],
        ),
        (
            "lister_devis",
            "Liste les devis/soumissions (pipeline de ventes).",
            &[("statut", "string"), ("client_company_id", "integer"), ("recherche", "string"), ("limite", "integer")],
            &[],
        ),
        (
            "suivi_lister_notes_calendrier",
            "Liste les notes libres du calendrier (post-it PARTAGES). Lecture.",
            &[("date_debut", "string"), ("date_fin", "string"), ("limite", "integer")],
            &[],
        ),
        ("banque_lister_comptes", "Liste les comptes bancaires lies.", &[("inclure_inactifs", "boolean")], &[]),
        ("conformite_expirations", "Elements de conformite qui expirent.", &[("jours", "integer")], &[]),
        (
            "lister_pointages",
            "Liste les entrees de pointage.",
            &[("employee_id", "integer"), ("date_debut", "string"), ("date_fin", "string"), ("limite", "integer")],
            &[],
        ),
        (
            "lister_bons_commande",
            "Liste les bons de commande (purchase orders).",
            &[("statut", "string"), ("limite", "integer")],
            &[],
        ),
        ("lister_bons_achat", "Liste les bons d'achat.", &[("statut", "string"), ("limite", "integer")], &[]),
        ("lister_livraisons", "Liste les livraisons de chantier.", &[("statut", "string"), ("limite", "integer")], &[]),
        ("whoami_tenant", "Le tenant de la connexion.", &[], &[]),
        ("lister_factures", "Liste les factures (statut, client, solde).", &[("statut", "string")], &[]),
        (
            "creer_facture",
            "Crée une facture CLIENT (statut BROUILLON) pour une entreprise cliente.",
            &[("client", "string"), ("montant", "number")],
            &["client", "montant"],
        ),
        ("supprimer_projet", "Annule un projet (annulation logique).", &[("project_id", "string")], &["project_id"]),
        (
            "enregistrer_paiement",
            "Enregistre un paiement sur une facture ENVOYÉE et poste l'écriture comptable.",
            &[("invoice_id", "integer"), ("montant", "number")],
            &["invoice_id", "montant"],
        ),
    ];

    /// Un faux Constructo AI, sur le poste seulement (127.0.0.1) : son serveur OAuth (`mcp_oauth.py`) et son connecteur
    /// MCP, pour essayer la connexion d'Eve sans jamais toucher au vrai. Il suit le contrat du vrai — l'enregistrement
    /// dynamique, PKCE S256 obligatoire, la `resource` exacte, un code à usage unique, un jeton de renouvellement qui
    /// TOURNE, et dont la réutilisation révoque toute la famille ; l'ancien jeton d'accès meurt au renouvellement —, mais
    /// sa page de connexion n'existe pas : `/authorize` accorde aussitôt (l'entreprise « tenant_essai », le compte
    /// info@exemple.ca). Repris du `faux_constructo.py` de l'édition Constructo d'Eve. Son connecteur parle comme celui
    /// du vrai (`StreamableHTTPSessionManager`, `stateless=False`, `json_response=False`) : une session par
    /// `initialize` (`Mcp-Session-Id`), exigée ensuite, fermée par un DELETE ; des réponses en SSE à qui les accepte ;
    /// et ses outils de lecture rendent les formes de `mcp_server.py`, sur une entreprise inventée. Son journal dit
    /// chaque requête, jamais un jeton.
    pub struct FauxConstructo {
        pub port: u16,
        etat: Arc<Mutex<Faux>>,
        /// Le sommeil du connecteur MCP : sa durée, puis, dès la première requête qui le réveille, l'heure du réveil.
        sommeil: Arc<Mutex<Sommeil>>,
    }

    /// Comme le vrai sur Render : la première requête réveille le serveur, et toutes attendent qu'il le soit.
    #[derive(Default)]
    struct Sommeil {
        duree: u64,
        reveil: Option<Instant>,
    }

    struct Faux {
        base: String,
        duree: u64,
        nombreux: usize,
        /// Les clients enregistrés, et leurs adresses de retour.
        clients: HashMap<String, Vec<String>>,
        /// Les codes : le client, l'adresse de retour, le défi de PKCE, et s'il a servi.
        codes: HashMap<String, (String, String, String, bool)>,
        /// Les jetons d'accès, et leur échéance.
        acces: HashMap<String, u64>,
        /// Les jetons de renouvellement : le client, et s'ils sont révoqués.
        renouvellements: HashMap<String, (String, bool)>,
        /// Les sessions MCP ouvertes ; les outils qui rendent le texte d'erreur du vrai ; « /mcp » qui renvoie vers
        /// « /mcp/ ».
        sessions: HashSet<String>,
        pannes: HashSet<String>,
        rediriger: bool,
        journal: Vec<String>,
    }

    /// Une réponse : son code, son type, ses en-têtes de plus, son corps.
    struct Reponse(u16, &'static str, Vec<(&'static str, String)>, Vec<u8>);

    fn en_json(code: u16, v: Value) -> Reponse {
        Reponse(code, "application/json", Vec::new(), v.to_string().into_bytes())
    }

    fn vide(code: u16) -> Reponse {
        Reponse(code, "text/plain", Vec::new(), Vec::new())
    }

    /// Le refus du point `/token`, dans les mots du vrai serveur.
    fn refus(description: &str) -> Reponse {
        en_json(400, json!({ "error": "invalid_grant", "error_description": description }))
    }

    impl FauxConstructo {
        /// Ouvre le faux serveur sur `port` (0 : un port libre), avec des jetons d'accès de `duree` secondes et
        /// `nombreux` lectures de remplissage.
        pub fn demarrer(port: u16, duree: u64, nombreux: usize) -> io::Result<FauxConstructo> {
            let ecoute = TcpListener::bind(("127.0.0.1", port))?;
            let port = ecoute.local_addr()?.port();
            let etat = Arc::new(Mutex::new(Faux {
                base: format!("http://127.0.0.1:{port}"),
                duree,
                nombreux,
                clients: HashMap::new(),
                codes: HashMap::new(),
                acces: HashMap::new(),
                renouvellements: HashMap::new(),
                sessions: HashSet::new(),
                pannes: HashSet::new(),
                rediriger: false,
                journal: vec![format!("démarrage : port {port}, des jetons de {duree} s")],
            }));
            let sommeil = Arc::new(Mutex::new(Sommeil::default()));
            let (partage, dort) = (etat.clone(), sommeil.clone());
            thread::spawn(move || {
                for flux in ecoute.incoming().flatten() {
                    let (etat, dort) = (partage.clone(), dort.clone());
                    thread::spawn(move || {
                        let _ = servir(flux, &etat, &dort);
                    });
                }
            });
            Ok(FauxConstructo { port, etat, sommeil })
        }

        /// Ce que le faux serveur a vu passer, une ligne par requête.
        pub fn journal(&self) -> Vec<String> {
            self.etat.lock().map(|e| e.journal.clone()).unwrap_or_default()
        }

        /// Le connecteur MCP s'endort, comme le vrai sur Render : la première requête qui lui arrive le réveille, et
        /// toutes attendent `secondes`, jusqu'à son réveil.
        pub fn endormir(&self, secondes: u64) {
            if let Ok(mut s) = self.sommeil.lock() {
                *s = Sommeil { duree: secondes, reveil: None };
            }
            if let Ok(mut f) = self.etat.lock() {
                f.noter(format!("endormi : le connecteur se réveillera {secondes} s après sa première requête"));
            }
        }

        /// Cet outil rend désormais le texte d'erreur du vrai (« Erreur lors de la lecture … (voir logs serveur). »).
        pub fn faire_echouer(&self, outil: &str) {
            if let Ok(mut f) = self.etat.lock() {
                f.pannes.insert(outil.to_string());
                f.noter(format!("panne : {outil} rendra une erreur"));
            }
        }

        /// « /mcp » renvoie désormais vers « /mcp/ » (un 307), comme le `Mount("/mcp")` de Starlette.
        pub fn rediriger(&self) {
            if let Ok(mut f) = self.etat.lock() {
                f.rediriger = true;
                f.noter("redirection : /mcp renvoie vers /mcp/".into());
            }
        }

        /// Les sessions MCP encore ouvertes.
        #[cfg(test)]
        pub fn sessions(&self) -> usize {
            self.etat.lock().map(|f| f.sessions.len()).unwrap_or(0)
        }
    }

    fn servir(mut flux: TcpStream, etat: &Mutex<Faux>, sommeil: &Mutex<Sommeil>) -> io::Result<()> {
        flux.set_read_timeout(Some(Duration::from_secs(15)))?;
        let Some(requete) = lire_requete(&mut flux)? else { return Ok(()) };
        let (chemin, parametres) = requete.cible.split_once('?').unwrap_or((requete.cible.as_str(), ""));
        // Endormi : une requête au connecteur attend son réveil (le verrou ne se tient pas pendant l'attente).
        let reveil = match sommeil.lock() {
            Ok(mut s) if chemin.starts_with("/mcp") && s.duree > 0 => {
                let duree = Duration::from_secs(s.duree);
                Some(*s.reveil.get_or_insert_with(|| Instant::now() + duree))
            }
            _ => None,
        };
        if let Some(r) = reveil.filter(|r| *r > Instant::now()) {
            thread::sleep(r.saturating_duration_since(Instant::now()));
            if let Ok(mut f) = etat.lock() {
                f.noter("une requête au connecteur a attendu son réveil".into());
            }
        }
        let reponse = match etat.lock() {
            Ok(mut f) => f.repondre(&requete, chemin, parametres),
            Err(_) => vide(500),
        };
        let Reponse(code, sorte, plus, corps) = reponse;
        let raison = match code {
            200 => "OK",
            201 => "Created",
            202 => "Accepted",
            302 => "Found",
            307 => "Temporary Redirect",
            400 => "Bad Request",
            401 => "Unauthorized",
            404 => "Not Found",
            405 => "Method Not Allowed",
            _ => "Error",
        };
        let mut tete = format!(
            "HTTP/1.1 {code} {raison}\r\nContent-Type: {sorte}\r\nContent-Length: {}\r\nConnection: close\r\n",
            corps.len()
        );
        for (nom, valeur) in plus {
            tete.push_str(&format!("{nom}: {valeur}\r\n"));
        }
        tete.push_str("\r\n");
        flux.write_all(tete.as_bytes())?;
        flux.write_all(&corps)?;
        flux.flush()
    }

    /// Une requête HTTP/1.1 reçue.
    struct Requete {
        methode: String,
        cible: String,
        /// Les en-têtes, leur nom en minuscules.
        entetes: Vec<(String, String)>,
        corps: Vec<u8>,
    }

    impl Requete {
        /// La valeur d'un en-tête (son nom en minuscules).
        fn entete(&self, nom: &str) -> Option<&str> {
            self.entetes.iter().find(|(n, _)| n == nom).map(|(_, v)| v.as_str())
        }
    }

    /// Lit une requête ; `None` si le client s'en va.
    fn lire_requete(flux: &mut TcpStream) -> io::Result<Option<Requete>> {
        let mut tampon = Vec::new();
        let mut morceau = [0u8; 4096];
        let fin = loop {
            let n = flux.read(&mut morceau)?;
            if n == 0 {
                return Ok(None);
            }
            tampon.extend_from_slice(&morceau[..n]);
            if let Some(p) = tampon.windows(4).position(|w| w == b"\r\n\r\n") {
                break p;
            }
            if tampon.len() > 64 * 1024 {
                return Ok(None);
            }
        };
        let tete = String::from_utf8_lossy(&tampon[..fin]).into_owned();
        let mut lignes = tete.split("\r\n");
        let mut premiere = lignes.next().unwrap_or("").split(' ');
        let methode = premiere.next().unwrap_or("").to_string();
        let cible = premiere.next().unwrap_or("/").to_string();
        let entetes: Vec<(String, String)> = lignes
            .filter_map(|l| l.split_once(':'))
            .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_string()))
            .collect();
        let longueur = entetes
            .iter()
            .find(|(n, _)| n == "content-length")
            .and_then(|(_, v)| v.parse::<usize>().ok())
            .unwrap_or(0)
            .min(1024 * 1024);
        let mut corps = tampon[fin + 4..].to_vec();
        while corps.len() < longueur {
            let n = flux.read(&mut morceau)?;
            if n == 0 {
                break;
            }
            corps.extend_from_slice(&morceau[..n]);
        }
        corps.truncate(longueur);
        Ok(Some(Requete { methode, cible, entetes, corps }))
    }

    /// Les champs d'un formulaire ou d'une requête (`a=1&b=2`), décodés.
    fn champs(texte: &str) -> HashMap<String, String> {
        texte.split('&').filter_map(|p| p.split_once('=')).map(|(n, v)| (decoder(n), decoder(v))).collect()
    }

    /// Un jeton de la forme d'un JWT, comme ceux du vrai serveur : son `sub` est le compte qu'affiche Eve.
    fn jwt() -> String {
        let charge = json!({ "sub": "info@exemple.ca", "tenant_schema": "tenant_essai", "jti": jeton_aleatoire() });
        format!("{}.{}.sig", b64url(br#"{"alg":"none"}"#), b64url(charge.to_string().as_bytes()))
    }

    impl Faux {
        fn noter(&mut self, ligne: String) {
            self.journal.push(ligne);
        }

        fn emettre(&mut self, client: &str) -> Value {
            let acces = jwt();
            self.acces.insert(acces.clone(), maintenant() + self.duree);
            let renouvellement = jeton_aleatoire();
            self.renouvellements.insert(renouvellement.clone(), (client.to_string(), false));
            json!({ "access_token": acces, "token_type": "Bearer", "expires_in": self.duree,
                    "refresh_token": renouvellement, "scope": "mcp" })
        }

        fn repondre(&mut self, requete: &Requete, chemin: &str, parametres: &str) -> Reponse {
            let corps = requete.corps.as_slice();
            match (requete.methode.as_str(), chemin) {
                ("POST", "/register") => self.enregistrer(corps),
                ("GET", "/authorize") => self.autoriser(parametres),
                ("POST", "/token") => {
                    let p = champs(&String::from_utf8_lossy(corps));
                    match p.get("grant_type").map(String::as_str).unwrap_or("") {
                        "authorization_code" => self.echanger(&p),
                        "refresh_token" => self.renouveler(p.get("refresh_token").map(String::as_str).unwrap_or("")),
                        autre => {
                            self.noter(format!("token refusé : grant_type « {autre} »"));
                            en_json(400, json!({ "error": "unsupported_grant_type" }))
                        }
                    }
                }
                // Le `Mount("/mcp")` de Starlette, quand on le lui demande : « /mcp » renvoie vers « /mcp/ ».
                (methode, "/mcp") if self.rediriger => {
                    self.noter(format!("{methode} /mcp renvoyé vers /mcp/"));
                    Reponse(307, "text/plain", vec![("Location", format!("{}/mcp/", self.base))], Vec::new())
                }
                ("POST", "/mcp" | "/mcp/") => self.mcp(requete),
                // Pas de flux SSE ouvert par le client : le transport HTTP de MCP le permet.
                ("GET", "/mcp" | "/mcp/") => vide(405),
                ("DELETE", "/mcp" | "/mcp/") => self.fermer_la_session(requete),
                _ => vide(404),
            }
        }

        /// Le jeton d'accès de la requête est-il le courant ? (L'ancien meurt au renouvellement.)
        fn porteur_valable(&self, requete: &Requete) -> bool {
            let jeton = requete.entete("authorization").and_then(|p| p.strip_prefix("Bearer ")).unwrap_or("");
            self.acces.get(jeton).is_some_and(|&fin| fin > maintenant())
        }

        /// Le DELETE qui ferme une session MCP (le jeton d'accès courant exigé, comme partout sur le connecteur).
        fn fermer_la_session(&mut self, requete: &Requete) -> Reponse {
            if !self.porteur_valable(requete) {
                return en_json(401, json!({ "error": "invalid_token" }));
            }
            match requete.entete("mcp-session-id") {
                Some(s) if self.sessions.remove(s) => {
                    self.noter("mcp DELETE : session fermée".into());
                    vide(200)
                }
                Some(_) => vide(404),
                None => vide(400),
            }
        }

        /// L'enregistrement dynamique d'un client public.
        fn enregistrer(&mut self, corps: &[u8]) -> Reponse {
            let v: Value = serde_json::from_slice(corps).unwrap_or(Value::Null);
            let retours: Vec<String> = v
                .get("redirect_uris")
                .and_then(Value::as_array)
                .map(|l| l.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            let id = format!("mcp_{}", &jeton_aleatoire()[..16]);
            self.noter(format!("register : {}", retours.join(", ")));
            self.clients.insert(id.clone(), retours.clone());
            en_json(201, json!({ "client_id": id, "redirect_uris": retours, "token_endpoint_auth_method": "none" }))
        }

        /// La page de connexion, qui accorde aussitôt : la redirection vers l'adresse de retour, avec le code.
        fn autoriser(&mut self, parametres: &str) -> Reponse {
            let q = champs(parametres);
            let c = |n: &str| q.get(n).map(String::as_str).unwrap_or("");
            let ressource = format!("{}/mcp", self.base);
            let retour_connu =
                self.clients.get(c("client_id")).is_some_and(|r| r.iter().any(|x| x == c("redirect_uri")));
            let ok = retour_connu
                && c("response_type") == "code"
                && c("code_challenge_method") == "S256"
                && !c("code_challenge").is_empty()
                && c("scope") == "mcp"
                && c("resource") == ressource;
            self.noter(format!("authorize {}", if ok { "accordé" } else { "refusé" }));
            if !ok {
                return vide(400);
            }
            let code = jeton_aleatoire();
            let entree =
                (c("client_id").to_string(), c("redirect_uri").to_string(), c("code_challenge").to_string(), false);
            self.codes.insert(code.clone(), entree);
            let cible = format!(
                "{}?code={}&state={}&iss={}",
                c("redirect_uri"),
                encoder(&code),
                encoder(c("state")),
                encoder(&self.base)
            );
            Reponse(302, "text/plain", vec![("Location", cible)], Vec::new())
        }

        /// Le code contre les jetons : le code n'a pas servi, l'adresse de retour et le client sont les siens, le
        /// vérificateur de PKCE répond au défi, la ressource est la bonne.
        fn echanger(&mut self, p: &HashMap<String, String>) -> Reponse {
            let c = |n: &str| p.get(n).map(String::as_str).unwrap_or("");
            let verifie = defi(c("code_verifier")).unwrap_or_default();
            let ressource = format!("{}/mcp", self.base);
            let ok = self.codes.get(c("code")).is_some_and(|(client, retour, attendu, servi)| {
                !servi
                    && retour == c("redirect_uri")
                    && client == c("client_id")
                    && *attendu == verifie
                    && c("resource") == ressource
            });
            self.noter(format!("token/code {}", if ok { "accordé" } else { "refusé" }));
            let client = match self.codes.get_mut(c("code")) {
                Some(entree) if ok => {
                    entree.3 = true;
                    entree.0.clone()
                }
                _ => return refus("Code d'autorisation invalide"),
            };
            en_json(200, self.emettre(&client))
        }

        /// Le renouvellement : le jeton TOURNE ; resservi, il révoque toute sa famille.
        fn renouveler(&mut self, jeton: &str) -> Reponse {
            let Some((client, revoque)) = self.renouvellements.get(jeton).cloned() else {
                self.noter("token/renouvellement refusé : jeton inconnu".into());
                return refus("Refresh token inconnu");
            };
            if revoque {
                for (de, r) in self.renouvellements.values_mut() {
                    if *de == client {
                        *r = true;
                    }
                }
                self.noter("token/renouvellement refusé : RÉUTILISÉ, toute la famille révoquée".into());
                return refus("Refresh token deja utilisé");
            }
            if let Some(r) = self.renouvellements.get_mut(jeton) {
                r.1 = true;
            }
            // L'ancien jeton d'accès meurt : Eve doit présenter le nouveau au connecteur.
            self.acces.clear();
            self.noter("token/renouvellement accordé".into());
            en_json(200, self.emettre(&client))
        }

        /// Le connecteur MCP, comme celui du vrai : le jeton d'accès courant (sinon 401) ; la session — `initialize` en
        /// ouvre une, toute autre requête doit porter la sienne (400 sans elle, 404 inconnue) ; les réponses en SSE à
        /// qui les accepte, sinon en JSON.
        fn mcp(&mut self, requete: &Requete) -> Reponse {
            let valable = self.porteur_valable(requete);
            let p: Value = serde_json::from_slice(&requete.corps).unwrap_or(Value::Null);
            let methode = p.get("method").and_then(Value::as_str).unwrap_or("").to_string();
            let outil = p.pointer("/params/name").and_then(Value::as_str).unwrap_or("").to_string();
            let quoi = if outil.is_empty() { methode.clone() } else { format!("{methode} {outil}") };
            self.noter(format!("mcp {quoi} ({})", if valable { "jeton courant" } else { "jeton refusé" }));
            if !valable {
                let mut r = en_json(401, json!({ "error": "invalid_token" }));
                r.2.push(("WWW-Authenticate", r#"Bearer error="invalid_token""#.to_string()));
                return r;
            }
            let mut plus = Vec::new();
            if methode == "initialize" {
                let session = jeton_aleatoire();
                self.sessions.insert(session.clone());
                plus.push(("mcp-session-id", session));
            } else {
                let erreur = |code: u16, message: &str| {
                    let e = json!({ "code": -32600, "message": message });
                    en_json(code, json!({ "jsonrpc": "2.0", "id": "server-error", "error": e }))
                };
                match requete.entete("mcp-session-id") {
                    None => return erreur(400, "Bad Request: Missing session ID"),
                    Some(s) if !self.sessions.contains(s) => return erreur(404, "Session not found"),
                    Some(_) => {}
                }
            }
            let Some(id) = p.get("id").cloned() else { return vide(202) };
            let resultat = match methode.as_str() {
                "initialize" => json!({
                    "protocolVersion": p.pointer("/params/protocolVersion").cloned().unwrap_or(json!("2025-06-18")),
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "faux-constructo", "version": "1" },
                }),
                "tools/list" => json!({ "tools": self.outils() }),
                "tools/call" => {
                    let a = p.pointer("/params/arguments").cloned().unwrap_or(Value::Null);
                    let (texte, erreur) = match self.appeler(&outil, &a) {
                        Ok(t) => (t, false),
                        Err(t) => (t, true),
                    };
                    json!({ "content": [{ "type": "text", "text": texte }], "isError": erreur })
                }
                _ => json!({}),
            };
            let message = json!({ "jsonrpc": "2.0", "id": id, "result": resultat });
            let mut r = if requete.entete("accept").is_some_and(|a| a.contains("text/event-stream")) {
                // sse-starlette : des fins de ligne `\r\n`, un événement par message.
                let flux = format!("event: message\r\ndata: {message}\r\n\r\n");
                Reponse(200, "text/event-stream", Vec::new(), flux.into_bytes())
            } else {
                en_json(200, message)
            };
            r.2.extend(plus);
            r
        }

        /// Un outil : son texte, ou son erreur (`isError`, comme FastMCP pour un outil qu'il n'a pas). Un outil en
        /// panne rend le texte d'erreur du vrai, sans `isError` (les outils de lecture attrapent tout).
        fn appeler(&self, outil: &str, a: &Value) -> Result<String, String> {
            if self.pannes.contains(outil) {
                return Ok("Erreur lors de la lecture (voir logs serveur).".into());
            }
            let x = |n: &str| a.get(n).map(|v| v.to_string().trim_matches('"').to_string()).unwrap_or_default();
            Ok(match outil {
                "whoami_tenant" => "Connecté au tenant : tenant_essai (par la connexion OAuth de l'entreprise).".into(),
                "creer_facture" => format!("Facture FACT-0160 créée pour {} ({} $).", x("client"), x("montant")),
                "supprimer_projet" => format!("Projet {} annulé.", x("project_id")),
                "enregistrer_paiement" => {
                    format!("Paiement de {} $ enregistré sur la facture {}.", x("montant"), x("invoice_id"))
                }
                o if o.starts_with("lister_registre_") => "Aucune entrée.".into(),
                o => entreprise_inventee(o, a).ok_or_else(|| format!("Unknown tool: {o}"))?,
            })
        }

        fn outils(&self) -> Vec<Value> {
            let mut liste: Vec<Value> = OUTILS
                .iter()
                .map(|(nom, description, proprietes, requis)| {
                    let p: serde_json::Map<String, Value> =
                        proprietes.iter().map(|(n, t)| (n.to_string(), json!({ "type": t }))).collect();
                    json!({ "name": nom, "description": description,
                            "inputSchema": { "type": "object", "properties": p, "required": requis } })
                })
                .collect();
            // Des lectures de remplissage, à la description longue comme celles du vrai serveur.
            for i in 0..self.nombreux {
                liste.push(json!({
                    "name": format!("lister_registre_{i}"),
                    "description": format!(
                        "Liste les entrées du registre nº {i} de l'entreprise : leur numéro, leur date, leur montant, leur \
                         statut et le projet auquel elles se rattachent. Filtre facultatif par statut ou par projet."
                    ),
                    "inputSchema": { "type": "object", "properties": { "statut": { "type": "string" } } },
                }));
            }
            liste
        }
    }

    /// Le jour à `decalage` jours d'aujourd'hui (« 2026-10-26 ») : les dates de l'entreprise inventée suivent l'essai.
    fn jour(decalage: i64) -> String {
        date_du_numero(numero_du_jour(&crate::journal::date()).unwrap_or(0) + decalage)
    }

    /// Le JSON d'un outil comme le rend `mcp_server.py` (`json.dumps(…, ensure_ascii=False, indent=2)`), derrière sa
    /// ligne d'en-tête s'il en a une (« 3 projet(s) trouve(s): »).
    fn rendu(entete: &str, v: &Value) -> String {
        let json = serde_json::to_string_pretty(v).unwrap_or_default();
        if entete.is_empty() { json } else { format!("{entete}\n{json}") }
    }

    /// Un statut tel que le compare le serveur : sans la casse (et sans `_` pour un projet).
    fn statut(v: &Value) -> String {
        v.as_str().unwrap_or("").to_lowercase().replace('_', " ")
    }

    /// Les lectures du tableau de bord d'Eve, sur une entreprise inventée (« Construction Exemple inc. »), dans les
    /// formes de `mcp_server.py` — ses filtres compris (le statut, les dates, la limite). `None` : un outil que le faux
    /// serveur n'a pas.
    fn entreprise_inventee(outil: &str, a: &Value) -> Option<String> {
        let limite = a["limite"].as_u64().unwrap_or(50) as usize;
        Some(match outil {
            "tableau_kpis" => rendu(
                "",
                &json!({
                    "projets_total": 14, "projets_en_cours": 3, "projets_termines": 9, "clients_actifs": 27,
                    "contacts_total": 61, "opportunites_ouvertes": 5, "pipeline_valeur": 612500.0, "devis_total": 38,
                    "devis_acceptes": 15, "factures_total": 112, "factures_solde_du": 48230.55,
                    "revenu_encaisse_periode": 73410.2, "employes_actifs": 8, "produits_actifs": 240,
                    "stock_critique": 2, "valeur_stock": 18450.75, "fournisseurs_actifs": 17, "bt_total": 44,
                    "devis_taux_conversion_pct": 39.5, "periode_jours": a["periode_jours"].as_u64().unwrap_or(30),
                }),
            ),
            "tableau_alertes" => {
                rendu("", &json!({ "factures_en_retard": { "nb": 3, "montant": 9714.92 }, "produits_sous_seuil": 2 }))
            }
            "tableau_factures_agees" => {
                let mut v = json!({
                    "tranches": [
                        { "tranche": "a_venir", "nb": 6, "montant": 38515.63 },
                        { "tranche": "0_30", "nb": 1, "montant": 2200.0 },
                        { "tranche": "31_60", "nb": 2, "montant": 7514.92 },
                    ],
                    "total_du": 48230.55,
                });
                if a["inclure_detail"].as_bool() == Some(true) {
                    let facture = |numero: &str, client: &str, jours: i64, solde: f64| {
                        json!({ "numero_facture": numero, "client_nom": client, "date_echeance": jour(-jours),
                                "solde_du": solde, "jours_retard": jours })
                    };
                    v["factures_en_retard"] = json!([
                        facture("F-2026-0142", "Construction Exemple inc.", 47, 5214.9),
                        facture("F-2026-0151", "Rénovations Exemple", 33, 2300.02),
                        facture("F-2026-0158", "Habitations Modèle inc.", 12, 2200.0),
                    ]);
                }
                rendu("", &v)
            }
            "tableau_pipeline_ventes" => rendu(
                "",
                &json!({
                    "par_statut": [
                        { "statut": "Proposition", "nb": 3, "valeur": 420000.0 },
                        { "statut": "GAGNE", "nb": 4, "valeur": 385000.0 },
                        { "statut": "Qualification", "nb": 2, "valeur": 192500.0 },
                        { "statut": "PERDU", "nb": 2, "valeur": 96000.0 },
                    ],
                    "valeur_totale": 1093500.0,
                }),
            ),
            "lister_projets" => {
                let projets = [
                    json!({ "id": 14, "nom_projet": "Agrandissement, 12 rue des Exemples",
                            "numero_projet": "PROJ-2026-00014", "client_company_id": 7,
                            "client_nom_cache": "Construction Exemple inc.", "statut": "En cours", "priorite": "Haute",
                            "budget_total": 425000.0, "prix_estime": null, "date_debut_reel": jour(-40),
                            "date_fin_reel": jour(50), "ville_chantier": "Exempleville" }),
                    json!({ "id": 12, "nom_projet": "Toiture, 8 chemin du Modèle", "numero_projet": "PROJ-2026-00012",
                            "client_nom_cache": null, "client_nom_direct": "Sam Exemple", "statut": "EN_COURS",
                            "priorite": "Moyenne", "budget_total": null, "prix_estime": 38000.0,
                            "date_debut_reel": jour(-10), "date_fin_reel": jour(8), "ville_chantier": "Exempleville" }),
                    json!({ "id": 9, "nom_projet": "Sous-sol, 3 rue de l'Essai", "numero_projet": "PROJ-2026-00009",
                            "client_nom_cache": "Rénovations Exemple", "statut": "En cours", "priorite": "Basse",
                            "budget_total": 72500.0, "date_debut_reel": jour(-75), "date_fin_reel": null,
                            "ville_chantier": "Démoville" }),
                    json!({ "id": 7, "nom_projet": "Cuisine, 5 avenue du Modèle", "numero_projet": "PROJ-2026-00007",
                            "statut": "Termine", "budget_total": 54000.0 }),
                ];
                let voulu = statut(&a["statut"]);
                let l: Vec<Value> = projets
                    .into_iter()
                    .filter(|p| voulu.is_empty() || statut(&p["statut"]) == voulu)
                    .take(limite)
                    .collect();
                rendu(&format!("{} projet(s) trouve(s):", l.len()), &Value::Array(l))
            }
            "lister_devis" => {
                let modifie = |decalage: i64| format!("{}T09:30:00", jour(decalage));
                let devis = [
                    json!({ "id": 35, "numero_devis": "DEV-2026-035", "nom_projet": "Véranda, 5 avenue du Modèle",
                            "client_nom_cache": "Construction Exemple inc.", "statut": "Brouillon",
                            "investissement_total": 0.0, "prix_estime": 26000.0, "date_soumis": null,
                            "created_at": modifie(-1), "updated_at": modifie(0) }),
                    json!({ "id": 33, "numero_devis": "DEV-2026-033", "nom_projet": "Terrasse, rang du Modèle",
                            "statut": "En attente", "investissement_total": null, "prix_estime": 12800.0,
                            "date_soumis": jour(-3), "updated_at": modifie(-2) }),
                    json!({ "id": 31, "numero_devis": "DEV-2026-031", "nom_projet": "Garage double, rue des Exemples",
                            "client_nom_cache": "Habitations Modèle inc.", "statut": "Envoye",
                            "investissement_total": 48000.0, "date_soumis": jour(-12), "updated_at": modifie(-12) }),
                    json!({ "id": 29, "numero_devis": "DEV-2026-029", "nom_projet": "Cuisine, avenue de l'Essai",
                            "client_nom_cache": null, "client_nom_direct": "Sam Exemple", "statut": "Envoye",
                            "investissement_total": 31500.0, "date_soumis": jour(-20), "updated_at": modifie(-15) }),
                    json!({ "id": 20, "numero_devis": "DEV-2026-020", "nom_projet": "Salle de bain, rue des Exemples",
                            "statut": "Accepte", "investissement_total": 22000.0, "date_soumis": jour(-60),
                            "updated_at": modifie(-40) }),
                ];
                let voulu = a["statut"].as_str().unwrap_or("").to_lowercase();
                let l: Vec<Value> = devis
                    .into_iter()
                    .filter(|d| voulu.is_empty() || d["statut"].as_str().unwrap_or("").to_lowercase() == voulu)
                    .take(limite)
                    .collect();
                rendu(&format!("{} devis trouve(s):", l.len()), &Value::Array(l))
            }
            "suivi_lister_notes_calendrier" => {
                let note = |id: u64, decalage: i64, contenu: &str| {
                    json!({ "id": id, "note_date": jour(decalage), "contenu": contenu, "scope": "partage",
                            "created_by_name": "Sam Exemple", "created_at": format!("{}T08:00:00", jour(-30)) })
                };
                let notes = [
                    note(40, 20, "Hors de la fenêtre : une note dans trois semaines"),
                    note(31, 6, "Livraison des fermes de toit"),
                    note(29, 1, "Visite de chantier, 12 rue des Exemples"),
                    note(27, 0, "Réunion de chantier, 8 h"),
                    note(20, -6, "Une note de la semaine passée"),
                ];
                let (debut, fin) = (a["date_debut"].as_str().unwrap_or(""), a["date_fin"].as_str().unwrap_or(""));
                let l: Vec<Value> = notes
                    .into_iter()
                    .filter(|n| {
                        let d = n["note_date"].as_str().unwrap_or("");
                        (debut.is_empty() || d >= debut) && (fin.is_empty() || d <= fin)
                    })
                    .take(limite)
                    .collect();
                rendu(&format!("{} note(s) partagee(s):", l.len()), &Value::Array(l))
            }
            "banque_lister_comptes" => "2 compte(s) bancaire(s) :\n\n\
                 #1 Compte courant (CHEQUE, GL 1010) ***4821\n\
                 \x20   solde publie : 51,240.18 $   solde banque : 52,010.43 $   ECART : 770.25 $\n\
                 \x20   7 operation(s) en attente\n\
                 #2 Marge de crédit (CREDIT, GL 2150)\n\
                 \x20   solde publie : -12,500.00 $   solde banque : inconnu (aucun releve importe)\n\
                 \x20   0 operation(s) en attente"
                .to_string(),
            "conformite_expirations" => rendu(
                "",
                &json!({
                    "date_reference": jour(0),
                    "jours": a["jours"].as_u64().unwrap_or(60),
                    "licences": [{ "id": 1, "numero_licence": "0000-0000-01",
                                   "nom_entreprise": "Construction Exemple inc.", "categories": ["1.2"],
                                   "date_emission": jour(-344), "date_expiration": jour(21), "statut": "ACTIVE" }],
                    "cartes": [],
                    "attestations": [{ "id": 3, "type": "CNESST", "numero": "A-0001", "date_emission": jour(-320),
                                       "date_expiration": jour(45), "statut": "VALIDE" }],
                    "total": 2,
                }),
            ),
            // Les factures, clients et fournisseurs mêlées : l'ERP garde le nom du fournisseur dans `client_nom`.
            "lister_factures" => {
                let facture =
                    |id: u64, nom: &str, projet: u64, destinataire: &str, decalage: i64, ttc: f64, solde: f64| {
                        let statut = if solde > 0.0 { "ENVOYEE" } else { "PAYEE" };
                        json!({ "id": id, "numero_facture": format!("FACT-2026-{id:05}"), "client_company_id": null,
                            "client_nom": nom, "project_id": projet, "type_document": "FACTURE",
                            "type_destinataire": destinataire, "date_facture": jour(decalage),
                            "date_echeance": jour(decalage + 30), "statut": statut, "montant_ttc": ttc,
                            "montant_total": ttc, "montant_paye": ttc - solde, "solde_du": solde,
                            "conditions_paiement": "Net 30", "date_paiement": null, "mode_paiement": null })
                    };
                let l: Vec<Value> = [
                    facture(212, "Béton Exemple ltée", 14, "fournisseur", -3, 3449.25, 3449.25),
                    facture(211, "Construction Exemple inc.", 14, "client", -4, 28743.75, 28743.75),
                    facture(207, "Quincaillerie Démo", 12, "fournisseur", -6, 412.5, 0.0),
                ]
                .into_iter()
                .take(limite)
                .collect();
                rendu(&format!("{} facture(s) trouvée(s):", l.len()), &Value::Array(l))
            }
            "lister_bons_commande" => {
                let l: Vec<Value> = [
                    json!({ "id": 5, "numero": "BC-2026-005", "numero_bon": null,
                            "fournisseur_nom": "Bois Exemple inc.", "projet_nom": "Agrandissement, 12 rue des Exemples",
                            "sous_total": 8000.0, "tps": 400.0, "tvq": 798.0, "total": 9198.0, "statut": "commande",
                            "date_commande": jour(-6), "date_livraison_prevue": jour(3),
                            "notes": "Fermes de toit, livrer avant 9 h" }),
                    json!({ "id": 4, "numero": "BC-2026-004", "numero_bon": null,
                            "fournisseur_nom": "Quincaillerie Démo", "projet_nom": null, "total": 640.0,
                            "statut": "recu", "date_commande": jour(-16), "date_livraison_prevue": jour(-11),
                            "notes": null }),
                ]
                .into_iter()
                .filter(|b| statut(&a["statut"]).is_empty() || statut(&b["statut"]) == statut(&a["statut"]))
                .take(limite)
                .collect();
                if l.is_empty() {
                    "0 bon(s) de commande trouvé(s):\nAucun(e) bon de commande trouvé(e).".to_string()
                } else {
                    rendu(&format!("{} bon(s) de commande trouvé(s):", l.len()), &Value::Array(l))
                }
            }
            "lister_bons_achat" => {
                let l: Vec<Value> =
                    [json!({ "id": 40, "numero_document": "BA-2026-040", "type_formulaire": "BON_ACHAT",
                                             "statut": "EN_ATTENTE", "priorite": "NORMAL", "montant_total": 1250.0,
                                             "date_creation": format!("{}T10:00:00", jour(-4)),
                                             "date_echeance": jour(2), "notes": "Revêtement de plancher du balcon",
                                             "projet_nom": "Agrandissement, 12 rue des Exemples",
                                             "fournisseur_nom": null })]
                    .into_iter()
                    .take(limite)
                    .collect();
                rendu(&format!("{} bon(s) d'achat trouvé(s):", l.len()), &Value::Array(l))
            }
            "lister_livraisons" => {
                let l: Vec<Value> =
                    [json!({ "id": 8, "reference": "LIV-2026-008", "project_id": 14, "fournisseur_id": 3,
                                             "date_prevue": jour(3), "heure_prevue": "08:30:00", "date_effective": null,
                                             "heure_effective": null, "statut": "planifiee",
                                             "type_livraison": "Matériaux", "zone_stockage": "Cour arrière",
                                             "notes": null })]
                    .into_iter()
                    .filter(|d| statut(&a["statut"]).is_empty() || statut(&d["statut"]) == statut(&a["statut"]))
                    .take(limite)
                    .collect();
                rendu(&format!("{} livraison(s):", l.len()), &Value::Array(l))
            }
            // Les pointages : la semaine passée (Sam du lundi au jeudi, Alex du lundi au vendredi), et aujourd'hui
            // (Alex a fini sa matinée, Sam est au travail) ; les bornes de l'appel sur `punch_in`.
            "lister_pointages" => {
                let n = numero_du_jour(&crate::journal::date()).unwrap_or(0);
                let lundi = -(n + 3).rem_euclid(7);
                // (numéro, employé, projet, jour, début, fin et heures : rien d'un pointage encore ouvert)
                type Pointage<'a> = (i64, (u64, &'a str, &'a str), u64, i64, &'a str, Option<(&'a str, f64)>);
                let pointage = |(id, qui, projet, decalage, debut, fin): Pointage<'_>| {
                    let j = jour(decalage);
                    json!({ "id": id, "employee_id": qui.0, "project_id": projet, "formulaire_bt_id": null,
                            "operation_id": null, "punch_in": format!("{j}T{debut}:00"),
                            "punch_out": fin.map(|(f, _)| format!("{j}T{f}:00")), "total_hours": fin.map(|(_, h)| h),
                            "type_travail": "normal", "notes": null, "validated": false, "billable": true,
                            "is_billed": false, "finalized_at": null, "prenom": qui.1, "nom_employe": qui.2 })
                };
                let (sam, alex) = ((2, "Sam", "Exemple"), (1, "Alex", "Modèle"));
                let mut tous = Vec::new();
                for k in 0..5 {
                    if k < 4 {
                        tous.push(pointage((400 + k, sam, 14, lundi - 7 + k, "07:00", Some(("15:00", 8.0)))));
                    }
                    tous.push(pointage((300 + k, alex, 9, lundi - 7 + k, "06:00", Some(("15:00", 9.0)))));
                }
                tous.push(pointage((501, alex, 14, 0, "06:30", Some(("10:00", 3.5)))));
                tous.push(pointage((502, sam, 12, 0, "07:00", None)));
                let (debut, fin) = (a["date_debut"].as_str().unwrap_or(""), a["date_fin"].as_str().unwrap_or(""));
                let mut l: Vec<Value> = tous
                    .into_iter()
                    .filter(|p| {
                        let d = p["punch_in"].as_str().and_then(|t| t.get(..10)).unwrap_or("");
                        (debut.is_empty() || d >= debut) && (fin.is_empty() || d <= fin)
                    })
                    .collect();
                l.sort_by(|x, y| y["punch_in"].as_str().cmp(&x["punch_in"].as_str()));
                l.truncate(limite.min(200));
                rendu(&format!("{} pointage(s) trouve(s):", l.len()), &Value::Array(l))
            }
            _ => return None,
        })
    }

    /// `GET url` sur le poste (`http://127.0.0.1:…` seulement), sans suivre de redirection : le code HTTP, et l'adresse
    /// où une redirection renvoie.
    pub fn demander_sans_suivre(url: &str) -> Result<(u16, Option<String>), String> {
        let reste = url.strip_prefix("http://127.0.0.1:").ok_or("adresse refusée : le poste seulement")?;
        let (port, chemin) = match reste.find('/') {
            Some(i) => (&reste[..i], &reste[i..]),
            None => (reste, "/"),
        };
        let port: u16 = port.parse().map_err(|_| "adresse refusée : port illisible".to_string())?;
        let mut f =
            TcpStream::connect(("127.0.0.1", port)).map_err(|e| format!("127.0.0.1:{port} ne répond pas ({e})"))?;
        let _ = f.set_read_timeout(Some(Duration::from_secs(60)));
        write!(f, "GET {chemin} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
            .map_err(|e| e.to_string())?;
        let mut r = Vec::new();
        f.read_to_end(&mut r).map_err(|e| e.to_string())?;
        let texte = String::from_utf8_lossy(&r);
        let statut = texte.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        let ou = texte.lines().take_while(|l| !l.is_empty()).find_map(|l| {
            let (nom, valeur) = l.split_once(':')?;
            nom.trim().eq_ignore_ascii_case("location").then(|| valeur.trim().to_string())
        });
        Ok((statut, ou))
    }

    /// Le navigateur des essais (`EVE_SANS_NAVIGATEUR`) : la page de connexion du faux serveur, puis la redirection vers
    /// la vitrine d'Eve, comme les suivrait le vrai — sur le poste seulement.
    pub fn jouer_le_navigateur(url: &str) -> Result<(), String> {
        let (statut, ou) = demander_sans_suivre(url)?;
        match ou {
            Some(ou) if (300..400).contains(&statut) && ou.starts_with("http://127.0.0.1:") => {
                let (fin, _) = demander_sans_suivre(&ou)?;
                if fin == 200 { Ok(()) } else { Err(format!("la vitrine a rendu HTTP {fin}")) }
            }
            _ => Err(format!("la page de connexion a rendu HTTP {statut}")),
        }
    }
}

// ---- La barre de titre que dessine Eve (cadre.rs) ----------------------------------------------------------------

/// `eve --essai-cadre [--captures DOSSIER]` : la vraie fenêtre native, sur un état inventé — ni chef, ni Claude, ni
/// micro, ni réseau ; Eve « écoute », et la musique du centre se tait —, et sa barre de titre mise à l'épreuve comme
/// Windows le ferait, sans la souris de la personne. Un fil interroge la fenêtre (`WM_NCHITTEST` : ses boutons, ce qui
/// reste à egui, la barre qui la déplace, le bord du haut qui la redimensionne, les autres bords) ; ouvre ses réglages
/// (le champ « Courriel » ; le panneau, par-dessus la barre, reste à egui) ; ouvre le menu de la fenêtre (le clic droit
/// sur la barre, Alt+Espace) et le referme ; lui poste les clics de ses boutons (agrandir : la zone client dans la zone
/// de travail de l'écran, « fermer » dans son coin ; restaurer ; réduire) ; la met en plein écran (la barre n'y déplace
/// plus rien) ; change son échelle, comme d'un écran à l'autre, et retour (elle ne doit pas grandir) ; la ferme par son
/// bouton. Une planche montre chaque bouton de la fenêtre dans chacun de ses états (leur survol, seule la vraie souris
/// le donne à la barre). La fenêtre s'ouvre sans prendre le clavier ; agrandie, en plein écran ou rendue, elle passe
/// devant un instant. `--captures` : ses images, par elle-même, en PNG, à chaque étape.
pub fn cadre(args: &[String]) -> i32 {
    #[cfg(windows)]
    {
        essai_du_cadre::essayer(args)
    }
    #[cfg(not(windows))]
    {
        let _ = args;
        eprintln!("eve --essai-cadre : la barre de titre d'Eve n'existe que sous Windows");
        2
    }
}

#[cfg(windows)]
mod essai_du_cadre {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use eframe::egui;
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    use windows::Win32::Graphics::Gdi::{
        ClientToScreen, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::{
        GUI_INMENUMODE, GUI_SYSTEMMENUMODE, GUITHREADINFO, GWL_STYLE, GetClientRect, GetForegroundWindow,
        GetGUIThreadInfo, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, HTBOTTOM, HTBOTTOMRIGHT, HTCAPTION,
        HTCLIENT, HTCLOSE, HTLEFT, HTMAXBUTTON, HTMINBUTTON, HTTOP, HTTOPLEFT, HTTOPRIGHT, IsIconic, IsWindow,
        IsWindowVisible, IsZoomed, PostMessageW, SC_RESTORE, SendMessageW, WM_CANCELMODE, WM_CLOSE, WM_DPICHANGED,
        WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCRBUTTONDOWN, WM_NCRBUTTONUP,
        WM_SYSCOMMAND, WM_SYSKEYDOWN, WM_SYSKEYUP, WS_CAPTION,
    };

    use super::enregistrer_png;
    use crate::cadre::{self, Bouton};
    use crate::chef::Evt;
    use crate::natif::{self, Allure, couleurs};

    /// Ce que le fil d'essai demande à la fenêtre, sur le fil de celle-ci.
    enum Ordre {
        /// Une capture de la fenêtre, et de quoi dire qu'elle est écrite.
        Capture(String, mpsc::Sender<()>),
        PleinEcran(bool),
        Reglages(bool),
    }

    /// La fenêtre de l'essai : la vraie fenêtre d'Eve, la planche des boutons par-dessus, et les ordres du fil d'essai.
    struct Essai {
        fenetre: natif::Fenetre,
        ordres: mpsc::Receiver<Ordre>,
        captures: Option<PathBuf>,
    }

    impl eframe::App for Essai {
        fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
            eframe::App::ui(&mut self.fenetre, ui, frame);
            let ctx = ui.ctx().clone();
            planche(&ctx);
            while let Ok(ordre) = self.ordres.try_recv() {
                match ordre {
                    Ordre::Capture(nom, fait) => match &self.captures {
                        Some(dossier) => {
                            let chemin = dossier.join(format!("cadre-{nom}.png"));
                            ctx.request_screenshot(move |image| {
                                match enregistrer_png(&image, &chemin) {
                                    Ok(()) => println!("        capture : {}", chemin.display()),
                                    Err(e) => eprintln!("        capture {} : {e}", chemin.display()),
                                }
                                let _ = fait.send(());
                            });
                        }
                        None => {
                            let _ = fait.send(());
                        }
                    },
                    Ordre::PleinEcran(oui) => ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(oui)),
                    Ordre::Reglages(oui) => self.fenetre.reglages_ouverts = oui,
                }
            }
            // Les ordres se lisent à chaque image : vingt par seconde au moins.
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
            eframe::App::on_exit(&mut self.fenetre, gl);
        }
    }

    /// La planche : chaque bouton de la fenêtre dans chacun de ses états, sur le marine de la barre.
    fn planche(ctx: &egui::Context) {
        let etats = [
            ("au repos", Allure::default()),
            ("survolé", Allure { survol: true, ..Allure::default() }),
            ("enfoncé", Allure { survol: true, enfonce: true, ..Allure::default() }),
            ("fenêtre inactive", Allure { inactive: true, ..Allure::default() }),
        ];
        let boutons = [
            ("Réduire", Bouton::Reduire, false),
            ("Agrandir", Bouton::Agrandir, false),
            ("Restaurer", Bouton::Agrandir, true),
            ("Fermer", Bouton::Fermer, false),
        ];
        egui::Area::new(egui::Id::unique("planche-des-boutons-de-la-fenetre"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(16.0, -16.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(couleurs::SURFACE)
                    .stroke((1.0, couleurs::FILET))
                    .corner_radius(10)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        let titre = "Les boutons de la fenêtre (la planche de l'essai)";
                        ui.label(egui::RichText::new(titre).size(12.0).color(couleurs::ENCRE_2));
                        egui::Grid::new("planche-des-boutons").spacing(egui::vec2(10.0, 6.0)).show(ui, |ui| {
                            ui.label("");
                            for (nom, _) in etats {
                                ui.label(egui::RichText::new(nom).size(11.5).color(couleurs::ENCRE_3));
                            }
                            ui.end_row();
                            for (nom, bouton, agrandie) in boutons {
                                ui.label(egui::RichText::new(nom).size(12.0).color(couleurs::ENCRE));
                                for (_, allure) in etats {
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(46.0, 32.0), egui::Sense::hover());
                                    ui.painter().rect_filled(r, 0.0, couleurs::BARRE_HAUT);
                                    natif::bouton_de_fenetre(ui.painter(), r, bouton, Allure { agrandie, ..allure });
                                }
                                ui.end_row();
                            }
                        });
                    });
            });
    }

    pub(super) fn essayer(args: &[String]) -> i32 {
        let captures = match args {
            [] => None,
            [option, dossier] if option == "--captures" => Some(PathBuf::from(dossier)),
            _ => {
                eprintln!("eve --essai-cadre [--captures DOSSIER]");
                return 2;
            }
        };
        if let Some(d) = &captures
            && let Err(e) = std::fs::create_dir_all(d)
        {
            eprintln!("{} : {e}", d.display());
            return 1;
        }
        // Une Eve inventée : son nom, une personne et une adresse d'exemple, deux lignes au fil.
        let etat = serde_json::json!({
            "etat": "ecoute",
            "libelle": "Je t'écoute",
            "aide": "Essai du cadre : la barre de titre d'Eve",
            "niveau": 0.0,
            "nom": "Essai du cadre",
            "qui": "Prénom",
            "propre": true,
            "oreille": "prete",
            "micro": "Micro d'essai",
            "fil": [
                { "de": "toi", "texte": "Montre-moi ta nouvelle barre.", "heure": "10 h 30", "pieces": [] },
                { "de": "eve", "texte": "La voici : glisse-la, double-clique-la, essaie ses boutons.",
                  "heure": "10 h 30", "pieces": [] },
            ],
            "voix": { "liste": [{ "id": "essai", "nom": "Voix d'essai" }], "choisie": "essai" },
            "reglages": {
                "eveil": false, "interruption": true, "sourdine": true, "devant": false, "demarrage": false,
                "historique": false, "raccourci": "Ctrl+Alt+E",
                "alertes": { "suspects": true, "factures": false, "pieces": false, "rendez_vous": false },
            },
            "eveil": false,
            "outils": [],
        });
        let (tx, rx) = mpsc::channel::<Evt>();
        let ecran = match crate::ecran::demarrer(0, crate::ecran::jeton_aleatoire(), tx.clone()) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("la vitrine de l'essai ne s'ouvre pas : {e}");
                return 1;
            }
        };
        ecran.publier(&etat.to_string());
        let abonnement = ecran.abonner();
        // Les gestes de la fenêtre, que le chef recevrait.
        let (vus, gestes) = mpsc::channel::<String>();
        thread::spawn(move || {
            for evt in rx {
                if let Evt::Ui { action, valeur } = evt {
                    println!("        geste : {action} {valeur}");
                    let _ = vus.send(action);
                }
            }
        });
        println!("La barre de titre d'Eve, mise à l'épreuve :");
        let (ordres_tx, ordres) = mpsc::channel::<Ordre>();
        let fil = thread::spawn(move || eprouver(&ordres_tx));
        let titre = format!("{} · Constructo AI", crate::fenetre::titre_pour("Essai du cadre"));
        let vue = egui::ViewportBuilder::default()
            .with_title(&titre)
            .with_inner_size([1240.0, 760.0])
            .with_min_inner_size([400.0, 540.0])
            .with_active(false);
        let options = eframe::NativeOptions { viewport: vue, ..Default::default() };
        let creer: eframe::AppCreator<'_> = Box::new(move |cc| {
            Ok(Box::new(Essai { fenetre: natif::Fenetre::nouvelle(cc, abonnement, tx, None), ordres, captures }))
        });
        if let Err(e) = eframe::run_native(&titre, options, creer) {
            eprintln!("la fenêtre ne s'ouvre pas : {e}");
            return 1;
        }
        drop(ecran);
        let mut bilan = fil.join().unwrap_or_else(|_| vec![("le fil d'essai va jusqu'au bout".to_string(), false)]);
        thread::sleep(Duration::from_millis(300));
        let quitter = gestes.try_iter().any(|g| g == "quitter");
        let quoi = "la fenêtre fermée dit « quitter » au chef";
        println!("  {} {quoi}", if quitter { "ok   " } else { "ÉCHEC" });
        bilan.push((quoi.to_string(), quitter));
        let echecs: Vec<&str> = bilan.iter().filter(|(_, ok)| !ok).map(|(q, _)| q.as_str()).collect();
        println!("{} vérifications, {} échec(s)", bilan.len(), echecs.len());
        for e in &echecs {
            println!("  échec : {e}");
        }
        if echecs.is_empty() { 0 } else { 1 }
    }

    /// Ce que l'essai a vérifié, dit au fur et à mesure.
    #[derive(Default)]
    struct Bilan(Vec<(String, bool)>);

    impl Bilan {
        fn noter(&mut self, quoi: &str, ok: bool, detail: &str) {
            let detail = if detail.is_empty() { String::new() } else { format!(" — {detail}") };
            println!("  {} {quoi}{detail}", if ok { "ok   " } else { "ÉCHEC" });
            self.0.push((quoi.to_string(), ok));
        }

        /// Ce que Windows trouve au point (x, y) de l'écran, et ce qu'il devait y trouver.
        fn toucher(&mut self, f: HWND, quoi: &str, (x, y): (i32, i32), attendu: u32) {
            let code = touche(f, x, y);
            let detail = if code == attendu {
                format!("{} en ({x}, {y})", nom_du_code(code))
            } else {
                format!("{} en ({x}, {y}), et non {}", nom_du_code(code), nom_du_code(attendu))
            };
            self.noter(quoi, code == attendu, &detail);
        }
    }

    /// Le fil d'essai : ce que Windows ferait de la barre, point par point.
    fn eprouver(ordres: &mpsc::Sender<Ordre>) -> Vec<(String, bool)> {
        let mut b = Bilan::default();
        let Some(f) = attendre(Duration::from_secs(20), || cadre::poignee().filter(|&f| visible(f))) else {
            b.noter(
                "la fenêtre s'ouvre, sa barre de titre est celle d'Eve",
                false,
                "aucune fenêtre sous-classée en 20 s",
            );
            return b.0;
        };
        // Quelques images : la barre se décrit.
        thread::sleep(Duration::from_millis(1500));
        // SAFETY : une lecture sur une fenêtre de ce programme.
        let fil = unsafe { GetWindowThreadProcessId(f, None) };
        let Some(barre) = cadre::barre() else {
            b.noter("la fenêtre décrit sa barre", false, "rien de décrit en 1,5 s");
            fermer(f);
            return b.0;
        };
        let f0 = fenetre(f);
        let c0 = client(f);
        let e = barre.echelle;
        let vu = cadre_visible(f).as_ref().map_or_else(|| "?".to_string(), texte);
        println!(
            "  fenêtre {} ; zone client {} ; cadre visible {vu} ; échelle {e} ; barre de {} points",
            texte(&f0),
            texte(&c0),
            barre.hauteur
        );
        b.noter("la zone client monte jusqu'en haut de la fenêtre", c0.top == f0.top, "");
        let bords = c0.left > f0.left && c0.right < f0.right && c0.bottom < f0.bottom;
        b.noter("Windows garde ses bords à gauche, à droite et en bas", bords, "");
        capturer(ordres, "rendue");

        // Ce que Windows trouve en chaque point de la barre et autour.
        let ecran_de = |c: &RECT, x: f32, y: f32| (c.left + (x * e).round() as i32, c.top + (y * e).round() as i32);
        let largeur = (c0.right - c0.left) as f32 / e;
        for (bouton, r) in &barre.boutons {
            let quoi = format!("le bouton {bouton:?} est celui de Windows");
            b.toucher(f, &quoi, ecran_de(&c0, r.center().x, r.center().y), code_du_bouton(*bouton));
        }
        for r in barre.client.iter().filter(|r| r.center().y < barre.hauteur) {
            let p = ecran_de(&c0, r.center().x, r.center().y);
            b.toucher(f, "ce qui se clique dans la barre reste à egui", p, HTCLIENT);
        }
        let milieu = ecran_de(&c0, largeur / 2.0, natif::BANDE * 0.6);
        b.toucher(f, "la bande, au milieu, déplace la fenêtre", milieu, HTCAPTION);
        b.toucher(
            f,
            "le site de Constructo AI se clique : il reste à egui",
            ecran_de(&c0, 40.0, natif::BANDE / 2.0),
            HTCLIENT,
        );
        b.toucher(f, "le logo aussi", ecran_de(&c0, 40.0, natif::BANDE + natif::RANGEE / 2.0), HTCAPTION);
        b.toucher(f, "le bord du haut redimensionne", ((c0.left + c0.right) / 2, f0.top + 1), HTTOP);
        b.toucher(f, "son coin de gauche", (c0.left + 1, f0.top + 1), HTTOPLEFT);
        b.toucher(f, "son coin de droite", (c0.right - 2, f0.top + 1), HTTOPRIGHT);
        if let Some((_, r)) = barre.boutons.iter().find(|(x, _)| *x == Bouton::Fermer) {
            let x = ecran_de(&c0, r.center().x, 0.0).0;
            b.toucher(f, "au-dessus du bouton fermer, le bord passe avant", (x, f0.top + 1), HTTOP);
        }
        b.toucher(f, "sous la barre, c'est egui", ecran_de(&c0, largeur / 2.0, barre.hauteur + 40.0), HTCLIENT);
        b.toucher(f, "le bord de gauche", (f0.left + 2, (f0.top + f0.bottom) / 2), HTLEFT);
        b.toucher(f, "le bord du bas", ((f0.left + f0.right) / 2, f0.bottom - 2), HTBOTTOM);
        b.toucher(f, "le coin en bas à droite", (f0.right - 2, f0.bottom - 2), HTBOTTOMRIGHT);

        // Les réglages ouverts : leur panneau déborde sur le bas de la barre, et ce qu'il en couvre reste à egui.
        let _ = ordres.send(Ordre::Reglages(true));
        thread::sleep(Duration::from_millis(700));
        let panneau = cadre::barre().and_then(|x| {
            let h = x.hauteur;
            x.client.into_iter().find(|r| r.top() < h && r.bottom() > h + 100.0).map(|r| (r, h))
        });
        b.noter("le panneau des réglages, par-dessus la barre, est décrit à Windows", panneau.is_some(), "");
        if let Some((r, h)) = panneau {
            let p = ecran_de(&c0, r.center().x, (r.top() + h) / 2.0);
            b.toucher(f, "ce qu'il couvre de la barre reste à egui", p, HTCLIENT);
        }
        capturer(ordres, "reglages");
        let _ = ordres.send(Ordre::Reglages(false));
        thread::sleep(Duration::from_millis(500));

        // Agrandir par son bouton, puis la rendre par le même ; puis le menu de la fenêtre.
        cliquer_sur(f, Bouton::Agrandir);
        // SAFETY (pour les attentes) : des lectures de l'état d'une fenêtre de ce programme.
        let agrandie = attendre_que(Duration::from_secs(3), || unsafe { IsZoomed(f) }.as_bool());
        b.noter("un clic sur « agrandir » l'agrandit", agrandie, "");
        if agrandie {
            thread::sleep(Duration::from_millis(900));
            let (c, travail) = (client(f), ecran(f).rcWork);
            let pres = |a: i32, b: i32| (a - b).abs() <= 2;
            let dedans = pres(c.left, travail.left)
                && pres(c.top, travail.top)
                && pres(c.right, travail.right)
                && pres(c.bottom, travail.bottom);
            let detail = format!(
                "zone client {}, zone de travail {}, fenêtre {}",
                texte(&c),
                texte(&travail),
                texte(&fenetre(f))
            );
            b.noter("agrandie, sa zone client est la zone de travail de l'écran", dedans, &detail);
            b.toucher(f, "agrandie, le coin de l'écran la ferme", (travail.right - 1, travail.top), HTCLOSE);
            let haut = ((travail.left + travail.right) / 2, travail.top);
            b.toucher(f, "agrandie, le haut de la barre la déplace", haut, HTCAPTION);
            capturer(ordres, "agrandie");
            cliquer_sur(f, Bouton::Agrandir);
            let rendue = attendre_que(Duration::from_secs(3), || !unsafe { IsZoomed(f) }.as_bool());
            b.noter("un second clic la rend", rendue, "");
            thread::sleep(Duration::from_millis(900));
            b.noter("rendue, elle reprend sa place", fenetre(f) == f0, &texte(&fenetre(f)));
            // Rendue après avoir été agrandie, la fenêtre est devant : le clavier lui va.
            menus(&mut b, f, fil, milieu);
        }

        // Le double clic sur la barre l'agrandit, et le suivant la rend : Windows le fait, sur HTCAPTION.
        poster(f, WM_NCLBUTTONDBLCLK, HTCAPTION as usize, point(milieu));
        let agrandie = attendre_que(Duration::from_secs(3), || unsafe { IsZoomed(f) }.as_bool());
        b.noter("un double clic sur la barre l'agrandit", agrandie, "");
        if agrandie {
            thread::sleep(Duration::from_millis(900));
            poster(f, WM_NCLBUTTONDBLCLK, HTCAPTION as usize, point(milieu_de_la_bande(f)));
            let rendue = attendre_que(Duration::from_secs(3), || !unsafe { IsZoomed(f) }.as_bool());
            b.noter("un second double clic la rend", rendue, "");
            thread::sleep(Duration::from_millis(900));
            b.noter("rendue, elle reprend sa place", fenetre(f) == f0, &texte(&fenetre(f)));
        }

        // Réduire par son bouton ; la rendre.
        cliquer_sur(f, Bouton::Reduire);
        let reduite = attendre_que(Duration::from_secs(3), || unsafe { IsIconic(f) }.as_bool());
        b.noter("un clic sur « réduire » la réduit", reduite, "");
        poster(f, WM_SYSCOMMAND, SC_RESTORE as usize, LPARAM(0));
        attendre_que(Duration::from_secs(3), || !unsafe { IsIconic(f) }.as_bool());
        thread::sleep(Duration::from_millis(900));
        b.noter("rendue, elle reprend sa place", fenetre(f) == f0, &texte(&fenetre(f)));

        // Le plein écran (la touche F) : winit ôte le cadre ; la barre ne déplace plus rien, et n'a plus de boutons.
        let _ = ordres.send(Ordre::PleinEcran(true));
        let plein = attendre_que(Duration::from_secs(3), || !encadree(f));
        b.noter("en plein écran, la fenêtre n'a plus de cadre", plein, "");
        if plein {
            thread::sleep(Duration::from_millis(900));
            let (c, moniteur) = (client(f), ecran(f).rcMonitor);
            b.noter("en plein écran, sa zone client est l'écran entier", c == moniteur, &texte(&c));
            b.toucher(f, "en plein écran, la barre ne la déplace plus", milieu_de_la_bande(f), HTCLIENT);
            let sans = cadre::barre().is_some_and(|x| x.boutons.is_empty());
            b.noter("en plein écran, la barre n'a plus de boutons de fenêtre", sans, "");
            capturer(ordres, "plein-ecran");
            let _ = ordres.send(Ordre::PleinEcran(false));
            attendre_que(Duration::from_secs(3), || encadree(f));
            thread::sleep(Duration::from_millis(900));
            b.noter("sortie du plein écran, elle reprend sa place", fenetre(f) == f0, &texte(&fenetre(f)));
        }

        // L'échelle, comme d'un écran à l'autre, et retour : la fenêtre ne grandit pas.
        // SAFETY : une lecture sur une fenêtre de ce programme.
        let dpi = unsafe { GetDpiForWindow(f) };
        let autre = if dpi == 96 { 144 } else { 96 };
        let changer = |d: u32| {
            let r = fenetre(f);
            let (w, l) = (WPARAM((d | (d << 16)) as usize), LPARAM((&raw const r) as isize));
            // SAFETY : un message envoyé (et attendu) à une fenêtre de ce programme ; `r` vit jusqu'à son retour.
            unsafe { SendMessageW(f, WM_DPICHANGED, Some(w), Some(l)) };
            thread::sleep(Duration::from_millis(900));
        };
        changer(autre);
        let entre = fenetre(f);
        changer(dpi);
        let apres = fenetre(f);
        let pres = |a: i32, b: i32| (a - b).abs() <= 2;
        let garde =
            pres(apres.right - apres.left, f0.right - f0.left) && pres(apres.bottom - apres.top, f0.bottom - f0.top);
        let detail = format!("à {autre} ppp {}, de retour à {dpi} ppp {}", texte(&entre), texte(&apres));
        b.noter("d'une échelle à l'autre et retour, elle garde sa taille", garde, &detail);
        capturer(ordres, "apres-l-echelle");

        // Fermer par son bouton.
        cliquer_sur(f, Bouton::Fermer);
        // SAFETY : une lecture ; une fenêtre détruite rend faux.
        let fermee = attendre_que(Duration::from_secs(5), || !unsafe { IsWindow(Some(f)) }.as_bool());
        b.noter("un clic sur « fermer » la ferme", fermee, "");
        if !fermee {
            fermer(f);
        }
        b.0
    }

    /// Le menu de la fenêtre : au clic droit sur la barre (Eve l'ouvre elle-même), puis à Alt+Espace — la touche
    /// seule, comme Windows la donne ; son caractère, Windows le fait (TranslateMessage), et la barre le lui rend.
    /// Windows n'ouvre ce menu au clavier qu'à la fenêtre qui est devant : sinon, Alt+Espace n'est pas vérifié.
    fn menus(b: &mut Bilan, f: HWND, fil: u32, milieu: (i32, i32)) {
        poster(f, WM_NCRBUTTONDOWN, HTCAPTION as usize, point(milieu));
        poster(f, WM_NCRBUTTONUP, HTCAPTION as usize, point(milieu));
        let ouvert = attendre_que(Duration::from_secs(3), || menu_ouvert(fil));
        b.noter("le clic droit sur la barre ouvre le menu de la fenêtre", ouvert, "");
        fermer_le_menu(f, fil);
        // SAFETY : une lecture.
        if unsafe { GetForegroundWindow() } != f {
            println!("  —     Alt+Espace : pas vérifié, la fenêtre n'est pas devant (le clavier n'est pas à elle)");
            return;
        }
        // Une frappe : la répétition 1, la touche d'espace (0x39), Alt tenu (le bit 29).
        let frappe = LPARAM(1 | (0x39 << 16) | (1 << 29));
        poster(f, WM_SYSKEYDOWN, 0x20, frappe);
        let ouvert = attendre_que(Duration::from_secs(3), || menu_ouvert(fil));
        b.noter("Alt+Espace ouvre le menu de la fenêtre", ouvert, "");
        fermer_le_menu(f, fil);
        poster(f, WM_SYSKEYUP, 0x20, LPARAM(frappe.0 | (1 << 30) | (1 << 31)));
        thread::sleep(Duration::from_millis(300));
    }

    /// Un clic sur un bouton de la fenêtre, comme Windows le donne à la fenêtre : enfoncé, relâché, sur son code ; à sa
    /// place dans la barre décrite.
    fn cliquer_sur(f: HWND, bouton: Bouton) {
        let Some(barre) = cadre::barre() else { return };
        let Some((_, r)) = barre.boutons.iter().find(|(b, _)| *b == bouton) else { return };
        let c = client(f);
        let p = (c.left + (r.center().x * barre.echelle) as i32, c.top + (r.center().y * barre.echelle) as i32);
        let code = code_du_bouton(bouton) as usize;
        poster(f, WM_NCLBUTTONDOWN, code, point(p));
        poster(f, WM_NCLBUTTONUP, code, point(p));
    }

    fn code_du_bouton(b: Bouton) -> u32 {
        match b {
            Bouton::Reduire => HTMINBUTTON,
            Bouton::Agrandir => HTMAXBUTTON,
            Bouton::Fermer => HTCLOSE,
        }
    }

    fn nom_du_code(code: u32) -> String {
        let nom = match code {
            HTCLIENT => "HTCLIENT",
            HTCAPTION => "HTCAPTION",
            HTTOP => "HTTOP",
            HTTOPLEFT => "HTTOPLEFT",
            HTTOPRIGHT => "HTTOPRIGHT",
            HTLEFT => "HTLEFT",
            HTBOTTOM => "HTBOTTOM",
            HTBOTTOMRIGHT => "HTBOTTOMRIGHT",
            HTMINBUTTON => "HTMINBUTTON",
            HTMAXBUTTON => "HTMAXBUTTON",
            HTCLOSE => "HTCLOSE",
            _ => return format!("le code {code}"),
        };
        nom.to_string()
    }

    /// Le milieu de la bande du haut, en pixels de l'écran, d'après la barre décrite.
    fn milieu_de_la_bande(f: HWND) -> (i32, i32) {
        let c = client(f);
        let e = cadre::barre().map_or(1.0, |x| x.echelle);
        ((c.left + c.right) / 2, c.top + (natif::BANDE * 0.6 * e) as i32)
    }

    fn visible(f: HWND) -> bool {
        // SAFETY : une lecture de l'état d'une fenêtre de ce programme.
        unsafe { IsWindowVisible(f) }.as_bool()
    }

    /// La fenêtre a son cadre (winit le lui ôte en plein écran).
    fn encadree(f: HWND) -> bool {
        // SAFETY : une lecture du style d'une fenêtre de ce programme.
        let style = unsafe { GetWindowLongW(f, GWL_STYLE) } as u32;
        style & WS_CAPTION.0 == WS_CAPTION.0
    }

    fn fenetre(f: HWND) -> RECT {
        let mut r = RECT::default();
        // SAFETY : une lecture ; `r` vit pendant l'appel.
        let _ = unsafe { GetWindowRect(f, &raw mut r) };
        r
    }

    /// La zone client, en pixels de l'écran.
    fn client(f: HWND) -> RECT {
        let (mut r, mut o) = (RECT::default(), POINT::default());
        // SAFETY : des lectures ; `r` et `o` vivent pendant les appels.
        unsafe {
            let _ = GetClientRect(f, &raw mut r);
            let _ = ClientToScreen(f, &raw mut o);
        }
        RECT { left: o.x, top: o.y, right: o.x + r.right, bottom: o.y + r.bottom }
    }

    /// Le cadre visible de la fenêtre, selon DWM (sans ses bords invisibles, qui débordent pour qu'on les attrape).
    fn cadre_visible(f: HWND) -> Option<RECT> {
        let mut r = RECT::default();
        // SAFETY : `r` vit pendant l'appel ; sa taille est celle qu'on annonce.
        unsafe { DwmGetWindowAttribute(f, DWMWA_EXTENDED_FRAME_BOUNDS, (&raw mut r).cast(), size_of::<RECT>() as u32) }
            .ok()
            .map(|()| r)
    }

    /// L'écran de la fenêtre : son rectangle, sa zone de travail.
    fn ecran(f: HWND) -> MONITORINFO {
        let mut i = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY : des lectures ; `i` vit pendant l'appel, sa taille est celle qu'on annonce.
        let _ = unsafe { GetMonitorInfoW(MonitorFromWindow(f, MONITOR_DEFAULTTONEAREST), &raw mut i) };
        i
    }

    fn texte(r: &RECT) -> String {
        format!("({}, {})–({}, {}) {}×{}", r.left, r.top, r.right, r.bottom, r.right - r.left, r.bottom - r.top)
    }

    /// Un point de l'écran, comme Windows le passe dans `lparam` : deux entiers de 16 bits, signés.
    fn point((x, y): (i32, i32)) -> LPARAM {
        LPARAM((isize::from(y as i16) << 16) | x as u16 as isize)
    }

    /// Ce que Windows trouve au point (x, y) de l'écran : la question qu'il pose à la fenêtre, et sa réponse.
    fn touche(f: HWND, x: i32, y: i32) -> u32 {
        // SAFETY : un message envoyé (et attendu) à une fenêtre de ce programme.
        unsafe { SendMessageW(f, WM_NCHITTEST, None, Some(point((x, y)))) }.0 as u32
    }

    fn poster(f: HWND, msg: u32, wparam: usize, lparam: LPARAM) {
        // SAFETY : un message posté à une fenêtre de ce programme.
        let _ = unsafe { PostMessageW(Some(f), msg, WPARAM(wparam), lparam) };
    }

    /// Le fil de la fenêtre est-il dans un menu (celui de la fenêtre) ?
    fn menu_ouvert(fil: u32) -> bool {
        let mut g = GUITHREADINFO { cbSize: size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        // SAFETY : une lecture ; `g` vit pendant l'appel, sa taille est celle qu'on annonce.
        let lu = unsafe { GetGUIThreadInfo(fil, &raw mut g) }.is_ok();
        lu && g.flags.0 & (GUI_INMENUMODE.0 | GUI_SYSTEMMENUMODE.0) != 0
    }

    /// Referme le menu de la fenêtre (WM_CANCELMODE : Windows abandonne le menu en cours).
    fn fermer_le_menu(f: HWND, fil: u32) {
        poster(f, WM_CANCELMODE, 0, LPARAM(0));
        attendre_que(Duration::from_secs(3), || !menu_ouvert(fil));
        thread::sleep(Duration::from_millis(300));
    }

    fn fermer(f: HWND) {
        poster(f, WM_CLOSE, 0, LPARAM(0));
    }

    /// Demande une capture à la fenêtre, et l'attend (cinq secondes au plus).
    fn capturer(ordres: &mpsc::Sender<Ordre>, nom: &str) {
        let (fait, ecrite) = mpsc::channel();
        if ordres.send(Ordre::Capture(nom.to_string(), fait)).is_ok() {
            let _ = ecrite.recv_timeout(Duration::from_secs(5));
        }
    }

    fn attendre<T>(delai: Duration, mut test: impl FnMut() -> Option<T>) -> Option<T> {
        let debut = Instant::now();
        loop {
            if let Some(v) = test() {
                return Some(v);
            }
            if debut.elapsed() >= delai {
                return None;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn attendre_que(delai: Duration, mut test: impl FnMut() -> bool) -> bool {
        attendre(delai, || test().then_some(())).is_some()
    }
}

// ---- Le thème « Constructo AI » de la fenêtre native (le lot 6) ---------------------------------------------------

/// Une scène de l'essai du thème : son nom, sa durée, la taille de la fenêtre (rien : celle qu'elle a).
struct SceneDuTheme {
    nom: &'static str,
    duree: f64,
    taille: Option<[f32; 2]>,
}

/// Les scènes de `eve --essai-theme`, dans l'ordre : le tableau de bord au repos (l'entreprise et la famille
/// ensemble, Constructo AI relié et ses chiffres à jour), Eve qui réfléchit, sa réponse (un tableau, qui devient une
/// carte), Eve qui la dit (une voix inventée, que suit la bouche du portrait), une demande d'accord, la fenêtre étroite
/// (la conversation seule) pendant qu'Eve écoute, et le tableau de bord de nouveau ; puis ses vues (l'entreprise, la
/// famille), une grande fenêtre (et le bas de sa page, défilée : « suite », « entreprise-suite ») et une moyenne, et
/// les états de l'entreprise : pas reliée, la page de connexion ouverte, la première lecture, une lecture qui n'a pas
/// abouti (avec des chiffres d'avant, puis sans), la liaison perdue (des chiffres d'hier) ; les cartes en ligne
/// ([`EnLigne`]) : l'ordinaire (le haut de la page, puis son bas), un incident (et des erreurs dans la dernière heure,
/// un paiement échoué : leurs cartes montent ; puis dans une fenêtre moyenne), des lectures manquées (pendant la
/// première lecture de l'entreprise : ses cartes n'y sont pas encore). Puis la vitrine (`--vitrine`), les moments
/// d'une même journée ([`MOMENTS_DE_LA_VITRINE`]) : la première attend le portrait et ses poses.
static SCENES_DU_THEME: [SceneDuTheme; 30] = [
    SceneDuTheme { nom: "tableau", duree: 6.0, taille: Some([1560.0, 900.0]) },
    SceneDuTheme { nom: "reflexion", duree: 4.0, taille: None },
    SceneDuTheme { nom: "reponse", duree: 5.0, taille: None },
    SceneDuTheme { nom: "parole", duree: 5.0, taille: None },
    SceneDuTheme { nom: "accord", duree: 5.0, taille: None },
    SceneDuTheme { nom: "etroite", duree: 5.0, taille: Some([980.0, 820.0]) },
    SceneDuTheme { nom: "retour", duree: 4.0, taille: Some([1560.0, 900.0]) },
    SceneDuTheme { nom: "entreprise", duree: 5.0, taille: None },
    SceneDuTheme { nom: "famille", duree: 5.0, taille: None },
    SceneDuTheme { nom: "grande", duree: 5.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "suite", duree: 4.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "entreprise-suite", duree: 4.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "moyenne", duree: 5.0, taille: Some([1180.0, 880.0]) },
    SceneDuTheme { nom: "non-reliee", duree: 5.0, taille: Some([1560.0, 900.0]) },
    SceneDuTheme { nom: "connexion", duree: 4.0, taille: None },
    SceneDuTheme { nom: "lecture", duree: 4.0, taille: None },
    SceneDuTheme { nom: "erreur", duree: 5.0, taille: None },
    SceneDuTheme { nom: "erreur-sans-chiffres", duree: 4.0, taille: None },
    SceneDuTheme { nom: "perdue", duree: 5.0, taille: None },
    SceneDuTheme { nom: "en-ligne", duree: 5.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "en-ligne-suite", duree: 4.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "en-ligne-incident", duree: 5.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "en-ligne-etroite", duree: 5.0, taille: Some([1180.0, 880.0]) },
    SceneDuTheme { nom: "en-ligne-manquee", duree: 5.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "vitrine-joie", duree: 14.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "vitrine-serieuse", duree: 7.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "vitrine-entreprise-suite", duree: 6.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "vitrine-empathie", duree: 7.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "vitrine-amusee", duree: 7.0, taille: Some([2100.0, 1180.0]) },
    SceneDuTheme { nom: "vitrine-famille", duree: 6.0, taille: Some([2100.0, 1180.0]) },
];

/// Le fichier qui dit que l'`entreprise.json` du dossier local est celui de l'essai du thème : l'essai le réécrit.
/// Sans lui, un `entreprise.json` est celui d'une vraie Eve (sa lecture de l'ERP) : rien n'y est écrit.
const MARQUE_DE_L_ENTREPRISE: &str = "essai-theme-entreprise.txt";

/// `eve --essai-theme [dossier] [--captures DOSSIER] [--secondes N] [--scenes A,B…] [--meteo] [--vitrine]` : la vraie
/// fenêtre native, dans le thème « Constructo AI », sur un état inventé que l'essai lui publie comme le chef le
/// ferait — une conversation, une réponse en tableau, une demande d'accord, Eve qui réfléchit puis écoute, la liaison
/// à Constructo AI et la vue du tableau de bord — et ses panneaux sur les données inventées de l'essai des panneaux
/// (`activite\*.js`), sans `famille.js` : ni météo ni aucune requête. Les services en ligne et Stripe aussi sont
/// inventés, imposés à la fenêtre ([`en_ligne_inventes`]) : ses suivis ne lisent rien. `--meteo` y met la famille
/// inventée de l'essai des panneaux, et sa ville : la météo se lit alors pour de vrai chez open-meteo. Ses scènes
/// défilent d'elles-mêmes (`--scenes` : seulement celles-là), puis la fenêtre reste au tableau de bord. `--vitrine` :
/// la journée de la vitrine ([`Vitrine`]), ses captures nommées d'après ses moments ; ni services en ligne ni Stripe
/// (son entreprise n'en a pas).
///
/// L'entreprise inventée (« Construction Exemple inc. », ses projets, ses factures, son pipeline) s'écrit dans
/// l'`entreprise.json` du dossier local que nomme `EVE_DOSSIER_LOCAL`, scène après scène, comme Eve l'écrirait en
/// lisant l'ERP : seulement si ce dossier est nommé (jamais celui d'office), et si son `entreprise.json` manque ou
/// vient de cet essai (sa marque à côté). Sinon, les scènes de l'entreprise montrent ce qui s'y trouve.
///
/// L'essai joue le chef : un texte écrit reçoit une réponse inventée, « Nouvelle conversation » vide le fil, l'accord
/// se donne ou se refuse, le thème se change dans les réglages (« Centre de commande » : la musique du tableau se ferait
/// entendre), la vue du tableau se choisit, « Relire » relit l'entreprise inventée, « Se connecter » la relie (en deux
/// secondes et demie), « Quitter Eve » ou la fermeture de la fenêtre l'arrêtent. `--captures` enregistre la fenêtre en
/// PNG à la fin de chaque scène, puis la ferme ; `--secondes N` la ferme d'elle-même. Le dossier (d'office
/// `%TEMP%\eve-essai-theme`) reçoit les données inventées ; un dossier dont le `activite\` vient d'ailleurs se lit tel
/// quel, et rien n'y est écrit.
pub fn theme(args: &[String]) -> i32 {
    let valeur = |nom: &str| args.iter().position(|a| a == nom).and_then(|i| args.get(i + 1)).cloned();
    let a_valeur = ["--captures", "--secondes", "--scenes"];
    let dossier = args
        .iter()
        .enumerate()
        .find(|(k, a)| !a.starts_with("--") && (*k == 0 || !a_valeur.contains(&args[k - 1].as_str())))
        .map_or_else(|| std::env::temp_dir().join("eve-essai-theme"), |(_, a)| std::path::PathBuf::from(a));
    let Some(jour) = crate::centre::numero_du_jour(&crate::journal::date()) else {
        eprintln!("la date du jour ne se lit pas");
        return 1;
    };
    let maintenant = crate::journal::horodatage().get(11..).unwrap_or("12-00-00").replace('-', ":");
    let seconde = seconde_du_jour(&maintenant).unwrap_or(12 * 3600);
    let activite = dossier.join("activite");
    let meteo = args.iter().any(|a| a == "--meteo");
    // La vitrine : ses scènes seulement, d'office ; ses données, dès qu'une de ses scènes est demandée.
    let vitrine_voulue = args.iter().any(|a| a == "--vitrine");
    let scenes: Vec<&SceneDuTheme> = match valeur("--scenes") {
        Some(noms) => {
            let voulues: Vec<&str> = noms.split(',').map(str::trim).collect();
            if let Some(inconnue) = voulues.iter().find(|n| !SCENES_DU_THEME.iter().any(|s| s.nom == **n)) {
                let toutes: Vec<&str> = SCENES_DU_THEME.iter().map(|s| s.nom).collect();
                eprintln!("scène inconnue : {inconnue} (les scènes : {})", toutes.join(", "));
                return 2;
            }
            SCENES_DU_THEME.iter().filter(|s| voulues.contains(&s.nom)).collect()
        }
        None => SCENES_DU_THEME.iter().filter(|s| s.nom.starts_with("vitrine") == vitrine_voulue).collect(),
    };
    let etrangeres = activite.is_dir() && !activite.join(MARQUE_DES_PANNEAUX).is_file();
    let vitrine = (vitrine_voulue || scenes.iter().any(|s| s.nom.starts_with("vitrine")))
        .then(|| crate::centre::numero_du_jour(JOUR_DE_LA_VITRINE))
        .flatten()
        .map(|jour| Vitrine { jour, activite: (!etrangeres).then(|| activite.clone()) });
    if etrangeres {
        println!("{} : des données qui ne sont pas de l'essai, lues telles quelles", activite.display());
    } else if let Some(v) = &vitrine {
        if let Err(e) = v.ecrire_la_famille() {
            eprintln!("{} : {e}", dossier.display());
            return 1;
        }
        println!("la vitrine (une famille, une entreprise, le {JOUR_DE_LA_VITRINE}) : {}", dossier.display());
    } else if let Err(e) = ecrire_les_panneaux_d_essai(&dossier, jour, seconde, !meteo) {
        eprintln!("{} : {e}", dossier.display());
        return 1;
    } else if meteo {
        println!("données inventées (celles de l'essai des panneaux, sa famille et sa ville) : {}", dossier.display());
    } else {
        println!("données inventées (celles de l'essai des panneaux, sans famille.js) : {}", dossier.display());
    }
    let entreprise = entreprise_de_l_essai();
    match &entreprise {
        Some(f) => println!("l'entreprise inventée : {}", f.display()),
        None => println!(
            "l'entreprise inventée ne s'écrit pas (EVE_DOSSIER_LOCAL non nommé, ou un entreprise.json qui n'est pas de \
             l'essai) : les scènes de l'entreprise montrent ce qui s'y trouve"
        ),
    }
    let captures = valeur("--captures").map(std::path::PathBuf::from);
    if let Some(c) = &captures
        && let Err(e) = std::fs::create_dir_all(c)
    {
        eprintln!("{} : {e}", c.display());
        return 1;
    }
    let secondes = valeur("--secondes").and_then(|s| s.parse::<f64>().ok());
    // La vitrine de l'essai, sur un port libre : la fenêtre s'y abonne comme à celle d'Eve ; ses gestes reviennent ici.
    let (rapport, gestes) = mpsc::channel::<Evt>();
    let ecran = match crate::ecran::demarrer(0, crate::ecran::jeton_aleatoire(), rapport.clone()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("la vitrine de l'essai ne s'ouvre pas : {e}");
            return 1;
        }
    };
    let mut etat = EtatDuTheme::nouveau(entreprise, vitrine);
    if let Some(premiere) = scenes.first() {
        etat.scene(premiere.nom);
    }
    ecran.publier(&etat.json());
    let abonnement = ecran.abonner();
    thread::spawn(move || mener_l_essai_du_theme(&ecran, &gestes, etat, &scenes, captures, secondes));
    if crate::natif::lancer(abonnement, rapport, "Eve — Essai du thème", Some(dossier)) {
        0
    } else {
        eprintln!("la fenêtre native ne s'ouvre pas (OpenGL)");
        1
    }
}

/// L'`entreprise.json` où l'essai du thème écrit son entreprise inventée : celui du dossier local que nomme
/// `EVE_DOSSIER_LOCAL`, s'il manque ou vient de l'essai (sa marque à côté, posée ici quand il manque) ; `None` sinon.
fn entreprise_de_l_essai() -> Option<std::path::PathBuf> {
    std::env::var_os("EVE_DOSSIER_LOCAL").filter(|d| !d.is_empty())?;
    let local = crate::reglages::dossier_local();
    let (fichier, marque) = (local.join(crate::centre::ENTREPRISE), local.join(MARQUE_DE_L_ENTREPRISE));
    if fichier.exists() && !marque.is_file() {
        return None;
    }
    let pose = "Cet entreprise.json est inventé par eve --essai-theme, qui le réécrit à chaque scène.\n";
    std::fs::write(&marque, pose).ok()?;
    Some(fichier)
}

/// L'heure du poste dans la forme de la lecture de l'ERP (RFC 3339, avec le fuseau du poste), il y a `secondes`.
fn heure_de_lecture(secondes: i64) -> String {
    let local = crate::journal::date_et_heure();
    let jour = crate::centre::numero_du_jour(&local).unwrap_or(0);
    let s = seconde_du_jour(local.get(11..).unwrap_or("12:00:00")).unwrap_or(12 * 3600);
    let civiles = jour * 86_400 + s;
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    // Le fuseau du poste, au quart d'heure.
    let fuseau = ((civiles - unix) as f64 / 900.0).round() as i64 * 900;
    let lu = civiles - secondes;
    let (j, s) = (lu.div_euclid(86_400), lu.rem_euclid(86_400));
    let signe = if fuseau < 0 { '-' } else { '+' };
    format!(
        "{}T{:02}:{:02}:{:02}{signe}{:02}:{:02}",
        crate::centre::date_du_numero(j),
        s / 3600,
        s / 60 % 60,
        s % 60,
        fuseau.abs() / 3600,
        fuseau.abs() / 60 % 60
    )
}

/// L'entreprise inventée de l'essai (personne de vrai : « Construction Exemple inc. », ses clients et ses chantiers
/// d'exemple), de la forme d'`entreprise.json` (celle qu'écrit la lecture de l'ERP) : son état, l'heure de sa lecture
/// (il y a `il_y_a` secondes), son erreur, et ses chiffres (`chiffres`), cohérents entre eux — ce qui est à recevoir
/// est la somme de la balance âgée, le retard celle des factures en retard, rangées dans leurs tranches, le pipeline
/// celle de ses étapes.
fn entreprise_inventee(etat: &str, il_y_a: i64, erreur: Option<&str>, chiffres: bool) -> Value {
    let mut v = serde_json::json!({ "etat": etat });
    if let Some(e) = erreur {
        v["erreur"] = e.into();
    }
    if !chiffres {
        return v;
    }
    let jour = crate::centre::numero_du_jour(&crate::journal::date()).unwrap_or(0);
    let d = |k: i64| crate::centre::date_du_numero(jour + k);
    v["lu_a"] = heure_de_lecture(il_y_a).into();
    v["kpis"] = serde_json::json!({
        "projets_en_cours": 7, "valeur_projets_en_cours": 2_131_500.0,
        "a_recevoir": 86_420.0, "en_retard": 23_150.0, "factures_en_retard": 4,
        "devis_en_attente": 6, "valeur_devis_en_attente": 312_800.0,
        "pipeline": 945_000.0, "opportunites_ouvertes": 9,
        "encaissements_30_jours": 132_640.0, "produits_sous_seuil": 3,
        "clients_actifs": 38, "employes_actifs": 9, "taux_conversion_devis": 41.5, "solde_banque": 57_310.0,
        "bons_a_commander": 2, "commandes_ouvertes": 2, "livraisons_prevues": 1
    });
    v["alertes"] = serde_json::json!([
        { "texte": "4 factures en retard : 23 150 $", "gravite": "haute", "sorte": "factures" },
        { "texte": "Heures manquantes le vendredi : Jo Démo", "gravite": "moyenne", "sorte": "heures" },
        { "texte": "2 cartes de compétence à renouveler d'ici 60 jours", "gravite": "moyenne",
          "sorte": "conformite" },
        { "texte": "3 produits sous le seuil de réapprovisionnement", "gravite": "moyenne", "sorte": "stock" },
        { "texte": "14 opérations bancaires à classer", "gravite": "info", "sorte": "banque" }
    ]);
    v["projets"] = serde_json::json!([
        { "nom": "Résidence", "adresse": "12 rue des Exemples", "numero": "PROJ-2026-00031",
          "client": "Construction Exemple inc.", "statut": "En cours", "valeur": 425_000.0, "ville": "Exempleville",
          "debut": d(-40), "fin": d(50) },
        { "nom": "Agrandissement", "adresse": "45 chemin du Test", "numero": "PROJ-2026-00029",
          "client": "Gestion Démo ltée", "statut": "En cours", "valeur": 318_500.0, "ville": "Villedémo",
          "debut": d(-62), "fin": d(18) },
        { "nom": "Rénovation de cuisine, 8 rue Fictive", "numero": "PROJ-2026-00027",
          "statut": "En cours", "valeur": 96_000.0, "ville": "Exempleville", "debut": d(-20), "fin": d(9) },
        { "nom": "Duplex, 3 avenue de l'Essai", "numero": "PROJ-2026-00024", "client": "Immeubles Fictifs inc.",
          "statut": "En cours", "valeur": 445_000.0, "ville": "Lac-Exemple", "debut": d(-95) },
        { "nom": "Garage double, 210 rang Démo", "numero": "PROJ-2026-00022", "client": "Client Exemple inc.",
          "statut": "En cours", "valeur": 88_000.0, "ville": "Saint-Démo", "debut": d(-30), "fin": d(25) },
        { "nom": "Sous-sol aménagé, 77 rue des Essais", "numero": "PROJ-2026-00019",
          "statut": "En cours", "valeur": 59_000.0, "ville": "Exempleville", "debut": d(-12), "fin": d(30) },
        { "nom": "Entrepôt, 1 boulevard de l'Exemple", "numero": "PROJ-2026-00015",
          "client": "Coopérative Exemple", "statut": "En cours", "valeur": 700_000.0, "ville": "Villedémo",
          "debut": d(-150), "fin": d(120) }
    ]);
    v["factures_en_retard"] = serde_json::json!([
        { "numero": "F-2026-0142", "client": "Gestion Démo ltée", "solde": 12_480.0, "jours": 67, "echeance": d(-67) },
        { "numero": "F-2026-0157", "client": "Client Exemple inc.", "solde": 6_230.5, "jours": 41, "echeance": d(-41) },
        { "numero": "F-2026-0163", "client": "Immeubles Fictifs inc.", "solde": 3_190.0, "jours": 18,
          "echeance": d(-18) },
        { "numero": "F-2026-0170", "client": "Rénovations Exemple", "solde": 1_249.5, "jours": 6, "echeance": d(-6) }
    ]);
    v["balance_agee"] = serde_json::json!({
        "a_venir": 63_270.0, "0_30": 4_439.5, "31_60": 6_230.5, "61_90": 12_480.0, "plus_de_90": 0.0,
        "total": 86_420.0
    });
    v["pipeline"] = serde_json::json!([
        { "etape": "Proposition", "nombre": 3, "valeur": 420_000.0 },
        { "etape": "Prospection", "nombre": 3, "valeur": 210_000.0 },
        { "etape": "Qualification", "nombre": 2, "valeur": 185_000.0 },
        { "etape": "Négociation", "nombre": 1, "valeur": 130_000.0 }
    ]);
    v["agenda"] = serde_json::json!([
        { "quand": d(0), "texte": "Visite de chantier, 12 rue des Exemples" },
        { "quand": d(1), "texte": "Livraison de béton, 45 chemin du Test" },
        { "quand": d(2), "texte": "Rencontre client : la soumission du garage double" },
        { "quand": d(4), "texte": "Inspection municipale, 3 avenue de l'Essai" }
    ]);
    v["devis"] = serde_json::json!([
        { "numero": "DEV-2026-041", "nom": "Agrandissement, 9 rue de l'Essai", "client": "Client Exemple inc.",
          "statut": "Envoye", "valeur": 96_400.0, "depuis": d(-5) },
        { "numero": "DEV-2026-038", "nom": "Toiture, 18 rang Démo", "client": "Gestion Démo ltée",
          "statut": "En attente", "valeur": 41_200.0, "depuis": d(-12) },
        { "numero": "DEV-2026-035", "nom": "Garage double, 210 rang Démo", "client": "Client Exemple inc.",
          "statut": "Envoye", "valeur": 88_000.0, "depuis": d(-19) },
        { "numero": "DEV-2026-031", "nom": "Galerie et escalier, 4 rue Fictive", "statut": "Envoye",
          "valeur": 23_700.0, "depuis": d(-26) },
        { "numero": "DEV-2026-027", "nom": "Salle de bain, 66 avenue de l'Exemple", "client": "Rénovations Exemple",
          "statut": "En attente", "valeur": 31_500.0, "depuis": d(-33) },
        { "numero": "DEV-2026-022", "nom": "Fondation, lot 12 du Test", "client": "Immeubles Fictifs inc.",
          "statut": "Envoye", "valeur": 32_000.0, "depuis": d(-47) }
    ]);
    v["banque"] = serde_json::json!([
        { "nom": "Compte courant", "solde": 48_910.0, "solde_publie": 47_380.0, "a_classer": 14 },
        { "nom": "Marge de crédit", "solde": 8_400.0, "solde_publie": 8_400.0, "a_classer": 0 },
        { "nom": "Compte en dollars américains", "solde_publie": 1_250.0, "a_classer": 0 }
    ]);
    // Les cartes du modèle : les soumissions, les factures reçues, les commandes et les livraisons, les heures — avec
    // des employés et des fournisseurs inventés.
    v["soumissions"] = serde_json::json!([
        { "numero": "DEV-2026-044", "nom": "Cuisine, 5 avenue du Modèle", "client": "Client Exemple inc.",
          "statut": "Brouillon", "valeur": 26_000.0, "depuis": d(-1), "modifie": d(0) },
        { "numero": "DEV-2026-041", "nom": "Agrandissement, 9 rue de l'Essai", "client": "Client Exemple inc.",
          "statut": "Envoye", "valeur": 96_400.0, "depuis": d(-5), "modifie": d(-5) },
        { "numero": "DEV-2026-038", "nom": "Toiture, 18 rang Démo", "client": "Gestion Démo ltée",
          "statut": "En attente", "valeur": 41_200.0, "depuis": d(-12), "modifie": d(-8) },
        { "numero": "DEV-2026-036", "nom": "Galerie et escalier, 4 rue Fictive", "statut": "Valide",
          "valeur": 23_700.0, "depuis": d(-14), "modifie": d(-10) }
    ]);
    v["factures_recues"] = serde_json::json!([
        { "numero": "FACT-2026-00214", "fournisseur": "Béton Exemple ltée", "projet": "Agrandissement",
          "date": d(-3), "montant": 3_449.25, "solde": 3_449.25, "statut": "ENVOYEE" },
        { "numero": "FACT-2026-00211", "fournisseur": "Bois Exemple inc.", "projet": "Résidence",
          "date": d(-4), "montant": 9_198.0, "solde": 0.0, "statut": "PAYEE" },
        { "numero": "FACT-2026-00207", "fournisseur": "Quincaillerie Démo", "projet": "Rénovation de cuisine",
          "date": d(-6), "montant": 612.4, "solde": 612.4, "statut": "ENVOYEE" }
    ]);
    v["commandes"] = serde_json::json!([
        { "sorte": "a_commander", "numero": "BA-2026-040", "projet": "Agrandissement", "statut": "EN_ATTENTE",
          "date": d(2), "montant": 1_250.0, "texte": "Revêtement de plancher du balcon" },
        { "sorte": "a_commander", "numero": "BA-2026-041", "projet": "Résidence", "statut": "EN_ATTENTE",
          "date": d(5), "texte": "Gypse pour le sous-sol" },
        { "sorte": "commande", "numero": "BC-2026-005", "fournisseur": "Bois Exemple inc.", "projet": "Résidence",
          "statut": "commande", "date": d(3), "montant": 9_198.0, "texte": "Fermes de toit, livrer avant 9 h" },
        { "sorte": "livraison", "numero": "LIV-2026-008", "projet": "Agrandissement", "statut": "planifiee",
          "date": d(1), "heure": "08:30", "texte": "Matériaux · cour arrière" }
    ]);
    let lundi = d(-((jour + 3).rem_euclid(7)));
    let semaine_passee = |k: i64| crate::centre::date_du_numero(jour - (jour + 3).rem_euclid(7) - 7 + k);
    // Les heures : celles d'aujourd'hui pour cette semaine (quel que soit le jour de l'essai), la semaine passée en
    // entier ; une absence vendredi dernier.
    v["heures"] = serde_json::json!({
        "semaine": lundi, "cette_semaine": 15.0, "semaine_passee": 126.5, "a_venir": 8.0, "employes_actifs": 3,
        "derniere_saisie": format!("{}T07:00", d(0)),
        "manquantes": { "jour": semaine_passee(4), "employes": ["Jo Démo"] },
        "employes": [
            { "nom": "Alex Modèle", "cette_semaine": 8.0, "semaine_passee": 45.0, "a_venir": 0.0, "au_travail": true,
              "jours": [ { "jour": semaine_passee(0), "heures": 9.0 }, { "jour": d(0), "heures": 8.0 } ] },
            { "nom": "Charlie Essai", "cette_semaine": 7.0, "semaine_passee": 41.0, "a_venir": 8.0,
              "au_travail": false,
              "jours": [ { "jour": semaine_passee(1), "heures": 8.5 }, { "jour": d(0), "heures": 7.0 } ] },
            { "nom": "Jo Démo", "cette_semaine": 0.0, "semaine_passee": 40.5, "a_venir": 0.0, "au_travail": false,
              "jours": [ { "jour": semaine_passee(0), "heures": 8.0 } ] }
        ],
        "par_projet": [
            { "nom": "Résidence", "numero": "PROJ-2026-00031", "heures": 8.0 },
            { "nom": "Agrandissement", "numero": "PROJ-2026-00029", "heures": 7.0 }
        ]
    });
    v
}

/// Ce que montrent les cartes en ligne de l'essai du thème : l'ordinaire (tout va, rien en erreur ; de vieilles
/// factures ouvertes, toutes échues, comme sur un vrai compte), un incident (chez GitHub, une maintenance chez Render,
/// des erreurs de Render dans la dernière heure, des réponses HTTP 502, un paiement échoué), des lectures manquées ; ou
/// rien (la vitrine : son entreprise n'a ni Render ni Stripe).
#[derive(Clone, Copy, PartialEq)]
enum EnLigne {
    Ordinaire,
    Incident,
    Manquee,
    Aucun,
}

/// Impose à la fenêtre de l'essai des services en ligne et un compte Stripe inventés (personne de vrai : les
/// ressources « app-exemple », « api-exemple », « base-exemple », des clients d'exemple, des dollars canadiens), lus
/// il y a deux et trois minutes : ses suivis ne lisent alors rien pour de vrai, dans aucune scène.
fn en_ligne_inventes(cas: EnLigne) {
    use crate::services::{self, Erreurs, Etat, Incident, Ligne, Niveau, PAGES, Releve, Reponses5xx, Sorte, Statut};
    use crate::stripe::{self, Abonnements, Issue, Montants, Ouvertes, Paiement, Somme, Virement};
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let il_y_a = |minutes: i64| unix - minutes * 60;
    let somme = |montant: f64, nombre: usize| Somme { montant, nombre, plus: false };
    if cas == EnLigne::Aucun {
        services::imposer(Releve { lu_a: unix, statuts: Vec::new(), render: Ok(Vec::new()) });
        stripe::imposer(Ok(Montants {
            lu_a: unix,
            devise: "cad".into(),
            disponible: 0.0,
            en_attente: 0.0,
            aujourdhui: Somme::default(),
            ce_mois: Somme::default(),
            abonnements: None,
            virement: None,
            ouvertes: None,
            paiements: Vec::new(),
        }));
        return;
    }
    let incident = |nom: &str, impact, statut: &str, depuis, composants: &[(&str, Niveau)]| {
        Ok(Etat {
            niveau: impact,
            incidents: vec![Incident {
                nom: nom.into(),
                impact,
                statut: statut.into(),
                depuis,
                lien: "https://stspg.io/exemple".into(),
            }],
            composants: composants.iter().map(|(c, n)| (c.to_string(), *n)).collect(),
        })
    };
    let statuts = PAGES
        .iter()
        .map(|(nom, page, _)| {
            let etat = match (cas, *nom) {
                (EnLigne::Incident, "GitHub") => incident(
                    "Degraded performance for Actions",
                    Niveau::Mineur,
                    "identified",
                    il_y_a(38),
                    &[("Actions", Niveau::Mineur), ("Pages", Niveau::Mineur)],
                ),
                (EnLigne::Incident, "Render") => incident(
                    "Scheduled maintenance of Postgres databases",
                    Niveau::Maintenance,
                    "in_progress",
                    il_y_a(25),
                    &[("Postgres", Niveau::Maintenance)],
                ),
                (EnLigne::Manquee, "Claude") => Err("la page d'état ne répond pas (délai dépassé)".into()),
                _ => Ok(Etat { niveau: Niveau::Operationnel, incidents: Vec::new(), composants: Vec::new() }),
            };
            Statut { nom: nom.to_string(), page: page.to_string(), etat }
        })
        .collect();
    let ressource = |nom: &str, sorte, nombre, derniere_heure, derniere: Option<(i64, &str)>| Erreurs {
        nom: nom.into(),
        sorte,
        nombre,
        plus: false,
        derniere_heure,
        derniere: derniere.map(|(quand, message)| Ligne { quand, message: message.into() }),
        reponses_5xx: Reponses5xx::default(),
    };
    // Deux réponses HTTP 502 d'un service web, que Render n'a pas mises au niveau « error ».
    let mut app = ressource("app-exemple", Sorte::Web, 0, 0, None);
    app.reponses_5xx = Reponses5xx {
        nombre: 2,
        plus: false,
        derniere_heure: 0,
        derniere: Some(Ligne { quand: il_y_a(3 * 60 + 5), message: "HTTP 502 · GET /api/devis".into() }),
    };
    let render = match cas {
        EnLigne::Manquee => Err("la clé de Render est refusée (HTTP 401) — reposer la clé : eve --cle render".into()),
        EnLigne::Incident => Ok(vec![
            app,
            ressource(
                "api-exemple",
                Sorte::Web,
                14,
                3,
                Some((
                    il_y_a(4),
                    "Error: connect ETIMEDOUT 10.0.0.12:5432 at TCPConnectWrap.afterConnect [as oncomplete] \
                     (node:net:1555:16) — GET /api/factures 500 in 30012 ms",
                )),
            ),
            ressource(
                "base-exemple",
                Sorte::Base,
                2,
                0,
                Some((il_y_a(7 * 60 + 12), "ERROR:  canceling statement due to statement timeout")),
            ),
        ]),
        _ => Ok(vec![
            ressource("app-exemple", Sorte::Web, 0, 0, None),
            ressource("api-exemple", Sorte::Web, 0, 0, None),
            ressource("base-exemple", Sorte::Base, 0, 0, None),
        ]),
    };
    services::imposer(Releve { lu_a: il_y_a(2), statuts, render });
    if cas == EnLigne::Manquee {
        stripe::imposer(Err(
            "la clé d'API est refusée (invalide, expirée ou révoquée) : la reposer par « eve --cle stripe »".into(),
        ));
        return;
    }
    let paiement = |montant: f64, minutes: i64, qui: &str, issue| Paiement {
        montant,
        quand: il_y_a(minutes),
        qui: qui.into(),
        issue,
    };
    let mut paiements = vec![
        paiement(199.0, 26, "Construction Exemple inc.", Issue::Reussi),
        paiement(149.0, 95, "Rénovations Démo", Issue::Reussi),
        paiement(249.0, 26 * 60, "Toitures Fictives ltée", Issue::Reussi),
        paiement(49.99, 30 * 60, "sam@exemple.ca", Issue::Rembourse),
        paiement(99.0, 50 * 60, "Gestion Exemple", Issue::Reussi),
    ];
    // Les factures ouvertes, de la forme de celles d'un vrai compte : des dizaines, toutes échues (de vieilles factures
    // que Stripe ne relance plus) ; un paiement d'office qui échoue en ajoute une.
    let mut ouvertes = Ouvertes { nombre: 23, plus: false, du: 4_870.0, echues: 23 };
    if cas == EnLigne::Incident {
        paiements.insert(0, paiement(249.0, 12, "Toitures Fictives ltée", Issue::Echoue));
        paiements.truncate(5);
        ouvertes = Ouvertes { nombre: 24, plus: false, du: 5_119.0, echues: 24 };
    }
    // Ce qui est entré aujourd'hui : les paiements réussis du jour, à l'heure du poste (un essai lancé après minuit
    // n'en a qu'un, ou aucun).
    let jour = crate::centre::numero_du_jour(&crate::journal::date()).unwrap_or(0);
    let local = jour * 86_400 + seconde_du_jour(&crate::journal::heure()).unwrap_or(12 * 3600);
    let du_jour: Vec<f64> = paiements
        .iter()
        .filter(|p| p.issue == Issue::Reussi && (p.quand + local - unix).div_euclid(86_400) == jour)
        .map(|p| p.montant)
        .collect();
    stripe::imposer(Ok(Montants {
        lu_a: il_y_a(3),
        devise: "cad".into(),
        disponible: 3_410.27,
        en_attente: 1_256.4,
        aujourdhui: somme(du_jour.iter().sum(), du_jour.len()),
        ce_mois: somme(8_921.5, 41),
        abonnements: Some(Abonnements { actifs: 27, mensuel: 4_263.0 }),
        virement: Some(Virement { montant: 2_184.6, arrivee: crate::centre::date_du_numero(jour + 2), fait: false }),
        ouvertes: Some(ouvertes),
        paiements,
    }));
}

// ---- La vitrine de l'essai du thème ------------------------------------------------------------------------------

/// Le jour de la vitrine : un vendredi d'automne, la veille de l'Halloween.
const JOUR_DE_LA_VITRINE: &str = "2026-10-30";

/// Un moment de la journée de la vitrine : sa scène, son heure (en secondes du jour), la vue du tableau de bord,
/// l'émotion d'Eve, combien de lignes de la conversation sont déjà là, le temps qu'il fait (la température, le code
/// WMO), les courriels non lus (la maison, le bureau ; l'un vient-il de l'école ou du CPE).
struct MomentDeLaVitrine {
    scene: &'static str,
    heure: i64,
    vue: &'static str,
    emotion: Option<&'static str>,
    lignes: usize,
    meteo: (f64, u32),
    non_lus: (u32, u32, bool),
}

/// La journée de la vitrine, dans l'ordre : le matin (le chantier de l'école, puis le CPE qui accepte la soumission :
/// la joie) ; l'avant-midi (qui doit le plus d'argent : le sérieux ; puis l'entreprise, le bas de sa page) ; le début
/// de l'après-midi (la petite qui fait de la fièvre : l'empathie) ; la fin de l'après-midi (le hockey du garçon :
/// l'amusement ; puis la famille).
const MOMENTS_DE_LA_VITRINE: [MomentDeLaVitrine; 6] = [
    MomentDeLaVitrine {
        scene: "vitrine-joie",
        heure: 8 * 3600 + 12 * 60,
        vue: "tout",
        emotion: Some("joie"),
        lignes: 4,
        meteo: (1.4, 0),
        non_lus: (2, 9, false),
    },
    MomentDeLaVitrine {
        scene: "vitrine-serieuse",
        heure: 10 * 3600 + 43 * 60,
        vue: "tout",
        emotion: Some("serieuse"),
        lignes: 6,
        meteo: (5.2, 1),
        non_lus: (2, 6, false),
    },
    MomentDeLaVitrine {
        scene: "vitrine-entreprise-suite",
        heure: 11 * 3600 + 20 * 60,
        vue: "entreprise",
        emotion: None,
        lignes: 6,
        meteo: (6.1, 1),
        non_lus: (2, 7, false),
    },
    MomentDeLaVitrine {
        scene: "vitrine-empathie",
        heure: 13 * 3600 + 38 * 60,
        vue: "tout",
        emotion: Some("empathie"),
        lignes: 9,
        meteo: (8.6, 2),
        non_lus: (3, 5, true),
    },
    MomentDeLaVitrine {
        scene: "vitrine-amusee",
        heure: 16 * 3600 + 22 * 60,
        vue: "tout",
        emotion: Some("amusee"),
        lignes: 11,
        meteo: (7.8, 2),
        non_lus: (1, 4, false),
    },
    MomentDeLaVitrine {
        scene: "vitrine-famille",
        heure: 16 * 3600 + 41 * 60,
        vue: "famille",
        emotion: None,
        lignes: 11,
        meteo: (7.1, 3),
        non_lus: (1, 4, false),
    },
];

/// Les lignes de la conversation de la vitrine, quand le rappel du hockey est posé : il est dans `rappels.json`.
const LIGNES_DU_RAPPEL: usize = 11;

/// Ce qui est à recevoir dans la vitrine : la balance âgée, additionnée.
const A_RECEVOIR: f64 = 2_380_000.0;

/// Les factures en retard de la vitrine, de la plus vieille échéance à la plus récente : le numéro, le client, le
/// solde, les jours de retard. Leur somme est le retard ; elles remplissent les tranches échues de la balance âgée.
const RETARDS_DE_LA_VITRINE: [(&str, &str, f64, i64); 6] = [
    ("F-2026-0402", "Développements Riveneuve inc.", 141_863.47, 64),
    ("F-2026-0411", "Immeubles Carrier-Dion", 22_781.35, 48),
    ("F-2026-0417", "Atelier Sorbier inc.", 45_096.20, 39),
    ("F-2026-0428", "Groupe médical des Méandres", 96_331.15, 21),
    ("F-2026-0433", "Usinage Vermeil inc.", 38_452.90, 12),
    ("F-2026-0441", "Entrepôts Giguère-Dumont inc.", 65_195.08, 5),
];

/// Les chantiers en cours de la vitrine, du plus récent au plus ancien : le numéro, le nom, le client, la valeur, la
/// fin prévue (dans combien de jours). Tous à Lévis.
const CHANTIERS_DE_LA_VITRINE: [(&str, &str, &str, f64, i64); 9] = [
    ("PROJ-2026-00137", "Condos Le Riveneuve, 24 unités", "Développements Riveneuve inc.", 4_800_000.0, 236),
    ("PROJ-2026-00135", "Entrepôt Giguère-Dumont, 3 400 m²", "Entrepôts Giguère-Dumont inc.", 3_150_000.0, 97),
    (
        "PROJ-2026-00133",
        "Agrandissement de l'école des Sentiers-Fleuris",
        "Centre de services scolaire des Deux-Lacs",
        2_650_000.0,
        84,
    ),
    ("PROJ-2026-00130", "Clinique médicale des Méandres", "Groupe médical des Méandres", 1_480_000.0, 64),
    ("PROJ-2026-00127", "Résidence Belleau", "Famille Belleau", 865_000.0, 119),
    ("PROJ-2026-00126", "Toiture industrielle, Usinage Vermeil", "Usinage Vermeil inc.", 520_000.0, 21),
    ("PROJ-2026-00121", "Aménagement commercial, Atelier Sorbier", "Atelier Sorbier inc.", 410_000.0, 9),
    ("PROJ-2026-00118", "Rénovation d'un triplex", "Immeubles Carrier-Dion", 340_000.0, 46),
    ("PROJ-2026-00109", "Garage d'entretien, Excavation Poulin-Roy", "Excavation Poulin-Roy inc.", 285_000.0, 68),
];

/// Les soumissions de la vitrine, la dernière modifiée d'abord : le numéro, le nom, le client, le statut de l'ERP, la
/// valeur, envoyée et modifiée il y a combien de jours. Celles qui attendent une réponse (« Envoye », « En attente »)
/// sont les devis en attente.
const SOUMISSIONS_DE_LA_VITRINE: [(&str, &str, &str, &str, f64, i64, i64); 13] = [
    ("DEV-2026-0219", "Agrandissement d'usine, 2 200 m²", "Usinage Vermeil inc.", "Brouillon", 2_450_000.0, 2, 0),
    ("DEV-2026-0216", "Immeuble de 12 logements", "Habitations Lagacé-Roy inc.", "Valide", 2_180_000.0, 5, 1),
    ("DEV-2026-0214", "Centre de la petite enfance, 80 places", "CPE Les Mésangeais", "Envoye", 1_450_000.0, 9, 3),
    ("DEV-2026-0211", "Résidence secondaire au lac", "Famille Paquin", "Envoye", 1_120_000.0, 10, 6),
    (
        "DEV-2026-0208",
        "Rénovation de bureaux, 3e étage",
        "Gestion Valcourt-Bédard inc.",
        "En attente",
        685_000.0,
        14,
        9,
    ),
    ("DEV-2026-0205", "Entrepôt frigorifique", "Distribution Côté-Morin inc.", "Envoye", 1_960_000.0, 16, 12),
    (
        "DEV-2026-0201",
        "Agrandissement de clinique dentaire",
        "Clinique dentaire Mercier-Bégin",
        "En attente",
        410_000.0,
        18,
        14,
    ),
    ("DEV-2026-0198", "Atelier de carrosserie", "Carrosserie Bélisle-Hamel inc.", "Envoye", 126_000.0, 21, 17),
    ("DEV-2026-0196", "Quatre jumelés", "Habitations Lagacé-Roy inc.", "En attente", 1_280_000.0, 23, 19),
    ("DEV-2026-0192", "Réfection de toiture, 2 400 m²", "Entrepôts Giguère-Dumont inc.", "Envoye", 248_000.0, 27, 24),
    ("DEV-2026-0189", "Salle de montre", "Machinerie Tardif-Hébert inc.", "En attente", 255_000.0, 30, 27),
    ("DEV-2026-0185", "Mezzanine et bureaux", "Entrepôts Giguère-Dumont inc.", "Envoye", 214_000.0, 34, 31),
    ("DEV-2026-0181", "Rénovation de chalet", "Famille Bérubé", "En attente", 152_000.0, 39, 36),
];

/// Le pipeline de la vitrine, ses étapes ouvertes par valeur : l'étape, le nombre d'opportunités, la valeur.
const PIPELINE_DE_LA_VITRINE: [(&str, u32, f64); 4] = [
    ("Proposition", 7, 6_900_000.0),
    ("Qualification", 6, 4_200_000.0),
    ("Prospection", 7, 3_100_000.0),
    ("Négociation", 3, 1_900_000.0),
];

/// L'équipe de la vitrine telle que la lecture la rend (vingt employés au plus, par nom) : le nom, ses heures de
/// lundi à jeudi, celles de la semaine passée. Deux n'ont rien saisi jeudi.
const EQUIPE_DE_LA_VITRINE: [(&str, [f64; 4], f64); 20] = [
    ("Alexis Tremblay", [9.0, 9.5, 9.0, 9.5], 46.5),
    ("Benoît Lachance", [8.0, 8.0, 8.5, 8.0], 40.0),
    ("Carl Dufresne", [8.0, 8.0, 8.0, 0.0], 40.0),
    ("David Thibodeau", [8.5, 8.0, 8.0, 8.5], 41.5),
    ("Émile Gauthier", [8.0, 8.0, 8.0, 8.0], 40.0),
    ("Francis Lavoie", [8.5, 8.5, 8.0, 0.0], 42.0),
    ("Guillaume Côté", [9.0, 8.5, 9.0, 8.5], 43.0),
    ("Jonathan Pelletier", [8.0, 8.0, 8.0, 8.5], 40.5),
    ("Kevin Simard", [8.0, 8.5, 8.0, 8.0], 40.0),
    ("Louis-Philippe Bergeron", [7.5, 8.0, 8.0, 8.0], 39.5),
    ("Marc-André Gagné", [8.0, 8.0, 8.5, 8.5], 41.0),
    ("Maxime Ouellet", [9.0, 9.0, 8.5, 9.0], 44.5),
    ("Nicolas Poulin", [8.0, 8.0, 8.0, 8.0], 40.0),
    ("Olivier Martel", [8.5, 8.0, 8.0, 8.0], 40.5),
    ("Patrick Girard", [8.0, 8.0, 7.5, 8.0], 39.0),
    ("Steve Boucher", [8.0, 8.5, 8.5, 8.0], 41.0),
    ("Tommy Bélanger", [8.0, 8.0, 8.0, 8.0], 38.5),
    ("Vincent Lapointe", [8.5, 8.5, 8.0, 8.5], 42.5),
    ("William Morin", [8.0, 8.0, 8.0, 8.0], 40.0),
    ("Yannick Leblanc", [8.0, 7.5, 8.0, 8.0], 39.5),
];

/// Les heures de la semaine par chantier (de lundi à jeudi : celles de vendredi se saisissent le soir), toute l'équipe
/// comprise : le chantier, son numéro, ses heures.
const HEURES_DE_LA_VITRINE: [(&str, &str, f64); 6] = [
    ("Condos Le Riveneuve, 24 unités", "PROJ-2026-00137", 412.0),
    ("Entrepôt Giguère-Dumont, 3 400 m²", "PROJ-2026-00135", 268.5),
    ("Agrandissement de l'école des Sentiers-Fleuris", "PROJ-2026-00133", 231.0),
    ("Clinique médicale des Méandres", "PROJ-2026-00130", 146.0),
    ("Résidence Belleau", "PROJ-2026-00127", 89.0),
    ("Toiture industrielle, Usinage Vermeil", "PROJ-2026-00126", 58.0),
];

/// La vitrine de `eve --essai-theme` (`--vitrine`, ou ses scènes) : la journée d'une entreprise et d'une famille
/// inventées mais vraisemblables, pour des captures à montrer — un entrepreneur général d'une vingtaine de millions par
/// année et d'une quarantaine d'employés, sa conjointe, un garçon de 6 ans en 1re année qui joue au hockey, une fille
/// de 4 ans au CPE qui fait de la danse. Personne de vrai : aucune entreprise connue ; la météo de Lévis, inventée
/// (sans requête). Tout se tient d'une capture à l'autre : les noms, les chiffres (le retard est la somme des factures
/// en retard, la balance âgée celle de ce qui est à recevoir, le pipeline celle de ses étapes, les heures celles des
/// chantiers), et l'heure, que la vitrine règle à chaque moment de sa journée ([`MOMENTS_DE_LA_VITRINE`]).
struct Vitrine {
    /// Le numéro du jour de la vitrine.
    jour: i64,
    /// Le `activite\` de l'essai, où s'écrivent sa famille et ses courriels ; `None` : un `activite\` d'ailleurs, où
    /// rien ne s'écrit.
    activite: Option<std::path::PathBuf>,
}

impl Vitrine {
    fn d(&self, k: i64) -> String {
        crate::centre::date_du_numero(self.jour + k)
    }

    /// Une date de la vitrine en toutes lettres, en minuscules : « jeudi 29 octobre ».
    fn en_lettres(&self, k: i64) -> String {
        crate::ambiance::date_longue(&self.d(k)).to_lowercase()
    }

    /// Le temps qu'il fait à un moment de la journée (`(température, code WMO)`), et la prévision de trois jours, lus
    /// comme une réponse d'open-meteo pour Lévis.
    fn bulletin(&self, (temperature, code): (f64, u32)) -> Option<crate::meteo::Bulletin> {
        let reponse = serde_json::json!({
            "current": { "temperature_2m": temperature, "weather_code": code },
            "daily": {
                "time": [self.d(0), self.d(1), self.d(2)],
                "snowfall_sum": [0.0, 0.0, 0.0],
                "wind_gusts_10m_max": [28.0, 41.0, 35.0],
                "temperature_2m_max": [9.4, 6.8, 4.1],
                "temperature_2m_min": [-0.6, 1.2, -2.3],
                "weather_code": [2, 61, 80],
                "precipitation_probability_max": [10, 60, 40]
            }
        });
        let lieu = crate::meteo::Lieu {
            ville: "Lévis".into(),
            latitude: 46.80,
            longitude: -71.18,
            fuseau: "America/Toronto".into(),
        };
        crate::meteo::lire(reponse.to_string().as_bytes(), &lieu, 10).ok()
    }

    /// La famille dans `activite\` : ses personnes et sa ville, ses échéances, sa décision du jour, ses questions, son
    /// épicerie, son souper, et les courriels du matin. La marque de l'essai des panneaux reste : l'essai les réécrit.
    fn ecrire_la_famille(&self) -> std::io::Result<()> {
        use serde_json::json;
        let Some(activite) = &self.activite else { return Ok(()) };
        std::fs::create_dir_all(activite)?;
        let ecrire = |nom: &str, quoi: &str, v: Value| std::fs::write(activite.join(nom), format!("{quoi} = {v};\n"));
        let marque = "Ces fichiers sont inventés par eve --essai-theme, qui les réécrit à chaque essai.\n";
        std::fs::write(activite.join(MARQUE_DES_PANNEAUX), marque)?;
        // Ni budget ni comptabilité dans la vitrine : celui de l'essai des panneaux s'en va.
        let budget = activite.join("budget.js");
        if budget.exists() {
            std::fs::remove_file(budget)?;
        }
        let famille = json!({
            "personnes": [
                { "nom": "Sam", "couleur": [0, 229, 255], "anniversaire": "1988-03-21" },
                { "nom": "Émilie", "couleur": [255, 122, 184], "anniversaire": "1990-12-04" },
                { "nom": "Félix", "couleur": [255, 210, 63], "motifs": ["hockey"], "anniversaire": "2020-02-11" },
                { "nom": "Rosalie", "couleur": [124, 255, 107], "motifs": ["CPE", "danse"],
                  "anniversaire": "2022-06-18" }
            ],
            "ecole": { "motifs": ["1re année"], "conge_de": "Félix" },
            "meteo": { "ville": "Lévis", "latitude": 46.80, "longitude": -71.18, "fuseau": "America/Toronto" },
            "cueillette": { "jours": [1, 2, 3, 4, 5], "debut": "15:20", "fin": "15:40",
                            "texte": "Va chercher Félix à l'école" }
        });
        ecrire("famille.js", "window.EVE_FAMILLE", famille)?;
        let echeances = json!([
            { "d": self.d(3), "q": "Journée pédagogique · pas d'école", "t": "ecole" },
            { "d": self.d(12), "q": "Vaccin des 4 ans de Rosalie, au CLSC", "t": "famille" },
            { "d": self.d(20), "q": "Rencontre de parents, 1re année", "t": "ecole" },
            { "d": self.d(46), "q": "Acompte provisionnel", "t": "fiscal" }
        ]);
        ecrire("echeances.js", "window.EVE_ECHEANCES", echeances)?;
        let decision = json!({
            "date": self.d(0), "avant": self.d(2), "texte": "Inscrire Félix au camp de la journée pédagogique",
            "source": "Le service de garde de l'école"
        });
        ecrire("decision.js", "window.EVE_DECISION", decision)?;
        let questions = json!([
            "Costume d'Halloween de Rosalie : licorne ou pompière ?",
            "Pointure des patins de Félix ?",
            "Qui garde les enfants le samedi 14 novembre ?"
        ]);
        ecrire("questions.js", "window.EVE_QUESTIONS", questions)?;
        let articles = [
            ("lait 2 %", false),
            ("bananes", false),
            ("pain de blé entier", false),
            ("yogourt à boire", false),
            ("œufs", true),
            ("cheddar", false),
            ("pommes Cortland", true),
            ("bonbons d'Halloween", false),
        ];
        let items: Vec<Value> = articles.iter().map(|(n, ok)| json!({ "n": n, "ok": ok })).collect();
        ecrire("epicerie.js", "window.EVE_EPICERIE", json!({ "maj": self.d(0), "items": items }))?;
        let souper = json!({ "date": self.d(0), "menu": "Pâté chinois", "sortir": "le bœuf haché du congélateur" });
        ecrire("souper.js", "window.EVE_SOUPER", souper)?;
        self.ecrire_les_courriels(MOMENTS_DE_LA_VITRINE[0].non_lus)
    }

    /// Les courriels non lus d'un moment (la maison, le bureau ; `ecole` : l'un vient de l'école ou du CPE) et l'agenda
    /// de la journée et du lendemain, comme les écrit le script d'Outlook (une marque d'ordre des octets en tête).
    fn ecrire_les_courriels(&self, (maison, bureau, ecole): (u32, u32, bool)) -> std::io::Result<()> {
        let Some(activite) = &self.activite else { return Ok(()) };
        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |x| x.as_secs());
        let boites = serde_json::json!({ "t": unix, "boites": [
            { "nom": "Maison", "non_lus": maison, "ecole": ecole },
            { "nom": "Bureau", "non_lus": bureau, "ecole": false }
        ] });
        let agenda = serde_json::json!({ "t": unix, "rdv": [
            { "d": self.d(0), "h": "09:30", "titre": "Appel avec la banque : la marge de crédit" },
            { "d": self.d(0), "h": "15:30", "titre": "Rencontre avec l'architecte, résidence Belleau" },
            { "d": self.d(0), "h": "16:00", "titre": "Réunion de coordination, entrepôt Giguère-Dumont" },
            { "d": self.d(0), "h": "17:30", "titre": "Hockey de Félix, à l'aréna" },
            { "d": self.d(1), "h": "09:00", "titre": "Cours de danse de Rosalie" },
            { "d": self.d(1), "h": "17:00", "titre": "Tournée d'Halloween avec les enfants" }
        ] });
        let texte = format!("\u{feff}window.EVE_OUTLOOK={boites};\nwindow.EVE_AGENDA={agenda};\n");
        std::fs::write(activite.join("outlook.js"), texte)
    }

    /// Le rappel du hockey, tel que `rappels.json` le garde.
    fn rappels(&self) -> Value {
        serde_json::json!([{
            "id": "vitrine-hockey", "a": format!("{} 17:00", self.d(0)), "texte": "Le hockey de Félix",
            "dit": false, "minuterie": false
        }])
    }

    /// L'entreprise de la vitrine, lue dans son ERP à `lu` (en secondes du jour de la vitrine, l'heure du poste sans
    /// fuseau), de la forme d'`entreprise.json` : ses chiffres viennent de ses listes, qui les additionnent.
    fn entreprise(&self, lu: i64) -> Value {
        use serde_json::json;
        let d = |k: i64| self.d(k);
        let argent = |v: f64| format!("{}\u{a0}$", crate::centre::milliers(v));
        let retard: f64 = RETARDS_DE_LA_VITRINE.iter().map(|f| f.2).sum();
        let tranche = |de: i64, a: i64| -> f64 {
            RETARDS_DE_LA_VITRINE.iter().filter(|f| (de..=a).contains(&f.3)).map(|f| f.2).sum()
        };
        let valeur: f64 = CHANTIERS_DE_LA_VITRINE.iter().map(|c| c.3).sum();
        let en_attente: Vec<_> =
            SOUMISSIONS_DE_LA_VITRINE.iter().filter(|s| matches!(s.3, "Envoye" | "En attente")).collect();
        let pipeline: f64 = PIPELINE_DE_LA_VITRINE.iter().map(|e| e.2).sum();
        let opportunites: u32 = PIPELINE_DE_LA_VITRINE.iter().map(|e| e.1).sum();
        let cette_semaine: f64 = HEURES_DE_LA_VITRINE.iter().map(|h| h.2).sum();
        let soumissions: Vec<Value> = SOUMISSIONS_DE_LA_VITRINE
            .iter()
            .map(|(numero, nom, client, statut, valeur, depuis, modifie)| {
                json!({ "numero": numero, "nom": nom, "client": client, "statut": statut, "valeur": valeur,
                        "depuis": d(-depuis), "modifie": d(-modifie) })
            })
            .collect();
        let lu_a = format!("{}T{:02}:{:02}:00", d(0), lu / 3600, lu / 60 % 60);
        json!({
            "lu_a": lu_a,
            "etat": "a_jour",
            "essai_a": lu_a,
            "kpis": {
                "projets_en_cours": CHANTIERS_DE_LA_VITRINE.len(), "valeur_projets_en_cours": valeur,
                "a_recevoir": A_RECEVOIR, "en_retard": retard, "factures_en_retard": RETARDS_DE_LA_VITRINE.len(),
                "devis_en_attente": en_attente.len(),
                "valeur_devis_en_attente": en_attente.iter().map(|s| s.4).sum::<f64>(),
                "pipeline": pipeline, "opportunites_ouvertes": opportunites,
                "encaissements_30_jours": 1_720_000.0, "produits_sous_seuil": 0,
                "clients_actifs": 64, "employes_actifs": 38, "taux_conversion_devis": 34.5,
                "solde_banque": 612_438.27,
                "bons_a_commander": 1, "commandes_ouvertes": 2, "livraisons_prevues": 3
            },
            "alertes": [
                { "texte": format!("{} factures en retard : {}", RETARDS_DE_LA_VITRINE.len(), argent(retard)),
                  "gravite": "haute", "sorte": "factures" },
                { "texte": "Licence RBQ à renouveler d'ici 45\u{a0}jours", "gravite": "moyenne",
                  "sorte": "conformite" },
                { "texte": "3 cartes de compétence CCQ à renouveler d'ici 60\u{a0}jours", "gravite": "moyenne",
                  "sorte": "conformite" },
                { "texte": format!("Heures manquantes le {} : Carl Dufresne, Francis Lavoie", self.en_lettres(-1)),
                  "gravite": "moyenne", "sorte": "heures" },
                { "texte": "9 opérations bancaires à classer", "gravite": "info", "sorte": "banque" }
            ],
            "projets": CHANTIERS_DE_LA_VITRINE.iter().map(|(numero, nom, client, valeur, fin)| json!({
                "nom": nom, "numero": numero, "client": client, "statut": "En cours", "valeur": valeur,
                "ville": "Lévis", "fin": d(*fin)
            })).collect::<Vec<_>>(),
            "factures_en_retard": RETARDS_DE_LA_VITRINE.iter().map(|(numero, client, solde, jours)| json!({
                "numero": numero, "client": client, "solde": solde, "jours": jours, "echeance": d(-jours)
            })).collect::<Vec<_>>(),
            "balance_agee": {
                "a_venir": A_RECEVOIR - retard, "0_30": tranche(1, 30), "31_60": tranche(31, 60),
                "61_90": tranche(61, 90), "plus_de_90": tranche(91, i64::MAX), "total": A_RECEVOIR
            },
            "pipeline": PIPELINE_DE_LA_VITRINE.iter().map(|(etape, nombre, valeur)| json!({
                "etape": etape, "nombre": nombre, "valeur": valeur
            })).collect::<Vec<_>>(),
            "agenda": [
                { "quand": d(0), "texte": "Réunion de chantier à 7 h, condos Le Riveneuve" },
                { "quand": d(3), "texte": "Livraison des fermes de toit, entrepôt Giguère-Dumont" },
                { "quand": d(4), "texte": "Inspection municipale, clinique médicale des Méandres" },
                { "quand": d(6), "texte": "Coulée de la dalle du gymnase, école des Sentiers-Fleuris" }
            ],
            "soumissions": soumissions,
            "banque": [
                { "nom": "Compte courant", "solde": 612_438.27, "solde_publie": 598_215.40, "a_classer": 9 },
                { "nom": "Marge de crédit", "solde": -350_000.0, "solde_publie": -350_000.0, "a_classer": 0 }
            ],
            "factures_recues": [
                { "numero": "FACT-2026-01482", "fournisseur": "Béton Rioux-Marquis inc.",
                  "projet": "Condos Le Riveneuve, 24 unités", "date": d(-1), "montant": 38_642.15, "solde": 38_642.15,
                  "statut": "ENVOYEE" },
                { "numero": "FACT-2026-01479", "fournisseur": "Armatures Belhumeur inc.",
                  "projet": "Agrandissement de l'école des Sentiers-Fleuris", "date": d(-2), "montant": 21_984.60,
                  "solde": 21_984.60, "statut": "ENVOYEE" },
                { "numero": "FACT-2026-01471", "fournisseur": "Fenestration Héroux-Lemire inc.",
                  "projet": "Clinique médicale des Méandres", "date": d(-4), "montant": 64_210.0, "solde": 64_210.0,
                  "statut": "ENVOYEE" },
                { "numero": "FACT-2026-01466", "fournisseur": "Structures Vallon inc.",
                  "projet": "Entrepôt Giguère-Dumont, 3 400 m²", "date": d(-5), "montant": 47_815.32, "solde": 0.0,
                  "statut": "PAYEE" },
                { "numero": "FACT-2026-01458", "fournisseur": "Location d'équipement Hamel-Ouellet",
                  "projet": "Condos Le Riveneuve, 24 unités", "date": d(-7), "montant": 6_948.75, "solde": 6_948.75,
                  "statut": "ENVOYEE" },
                { "numero": "FACT-2026-01452", "fournisseur": "Matériaux Gendreau inc.", "projet": "Résidence Belleau",
                  "date": d(-8), "montant": 12_377.04, "solde": 4_377.04, "statut": "PARTIELLEMENT_PAYEE" }
            ],
            "commandes": [
                { "sorte": "a_commander", "numero": "BA-2026-0311", "projet": "Condos Le Riveneuve, 24 unités",
                  "statut": "EN_ATTENTE", "date": d(5), "montant": 186_400.0, "texte": "Portes et fenêtres, phase 2" },
                { "sorte": "commande", "numero": "BC-2026-0207", "fournisseur": "Armatures Belhumeur inc.",
                  "projet": "Agrandissement de l'école des Sentiers-Fleuris", "statut": "commande", "date": d(4),
                  "montant": 31_640.0, "texte": "Acier d'armature, dalle du gymnase" },
                { "sorte": "commande", "numero": "BC-2026-0205", "fournisseur": "Structures Vallon inc.",
                  "projet": "Entrepôt Giguère-Dumont, 3 400 m²", "statut": "commande", "date": d(3),
                  "montant": 88_900.0, "texte": "Fermes de toit, 34 unités" },
                { "sorte": "livraison", "numero": "LIV-2026-0439", "projet": "Entrepôt Giguère-Dumont, 3 400 m²",
                  "statut": "planifiee", "date": d(3), "heure": "08:00", "texte": "Fermes de toit · entrée nord" },
                { "sorte": "livraison", "numero": "LIV-2026-0442",
                  "projet": "Agrandissement de l'école des Sentiers-Fleuris", "statut": "planifiee", "date": d(6),
                  "heure": "06:30", "texte": "Béton 32 MPa, 48 m³ · dalle du gymnase" },
                { "sorte": "livraison", "numero": "LIV-2026-0445", "projet": "Condos Le Riveneuve, 24 unités",
                  "statut": "planifiee", "date": d(7), "heure": "07:30", "texte": "Portes et fenêtres, 3e étage" }
            ],
            "heures": {
                "semaine": d(-4), "cette_semaine": cette_semaine, "semaine_passee": 1_482.0, "a_venir": 0.0,
                "employes_actifs": 38, "derniere_saisie": format!("{}T17:46", d(-1)),
                "manquantes": { "jour": d(-1), "employes": ["Carl Dufresne", "Francis Lavoie"] },
                "employes": EQUIPE_DE_LA_VITRINE.iter().map(|(nom, jours, passee)| {
                    let saisis: Vec<Value> = jours.iter().enumerate().filter(|(_, h)| **h > 0.0)
                        .map(|(k, h)| json!({ "jour": d(k as i64 - 4), "heures": h })).collect();
                    json!({ "nom": nom, "cette_semaine": jours.iter().sum::<f64>(), "semaine_passee": passee,
                            "a_venir": 0.0, "au_travail": false, "jours": saisis })
                }).collect::<Vec<_>>(),
                "par_projet": HEURES_DE_LA_VITRINE.iter().map(|(nom, numero, heures)| json!({
                    "nom": nom, "numero": numero, "heures": heures
                })).collect::<Vec<_>>()
            }
        })
    }

    /// La conversation de la journée, ligne après ligne, comme le chef la publie : le texte que montre la fenêtre (son
    /// émotion déjà ôtée ; ses cartes après les trois tirets) et, sous chaque réponse de Claude, ce que son tour a
    /// pris. Elle dit ce que montre le tableau de bord : le chantier de l'école, la plus grosse facture en retard, le
    /// hockey.
    fn conversation(&self) -> Vec<Value> {
        let ligne = |de: &str, texte: &str, heure: &str| EtatDuTheme::ligne(de, texte, heure, false);
        let eve = |texte: &str, heure: &str, tour: &str| {
            let mut l = ligne("eve", texte, heure);
            l["tour"] = Value::String(format!("{tour} · Sonnet 5.5, moyen"));
            l
        };
        // Un montant au cent, comme Claude l'écrit dans une carte : « 141 863,47 $ ».
        let argent = |v: f64| {
            let cents = (v * 100.0).round() as i64;
            format!("{},{:02}\u{a0}$", crate::centre::milliers((cents / 100) as f64), cents % 100)
        };
        let mut retards: Vec<_> = RETARDS_DE_LA_VITRINE.iter().collect();
        retards.sort_by(|a, b| b.2.total_cmp(&a.2));
        let lignes: Vec<Value> = retards
            .iter()
            .map(|(_, client, solde, jours)| serde_json::json!([client, format!("{jours} j"), argent(*solde)]))
            .collect();
        let total: f64 = RETARDS_DE_LA_VITRINE.iter().map(|f| f.2).sum();
        let tableau = serde_json::json!({
            "type": "tableau", "titre": "Factures en retard", "colonnes": ["Client", "Retard", "Solde"],
            "lignes": lignes, "pied": ["Total", "", argent(total)]
        });
        let premiere = RETARDS_DE_LA_VITRINE[0];
        let rappel = serde_json::json!({ "type": "rappel", "a": format!("{} 17:00", self.d(0)),
                                         "texte": "Le hockey de Félix" });
        let dette = format!(
            "{} : environ {} 000 $, en retard de {} jours. En tout, six factures sont en retard, pour à peu près \
             410 000 $. Veux-tu que je prépare une relance ?\n---\n```carte\n{tableau}\n```",
            premiere.1.trim_end_matches(" inc."),
            (premiere.2 / 1000.0).round(),
            premiere.3
        );
        vec![
            ligne("toi", "Où en est le chantier de l'école ?", "07:52"),
            eve(
                "L'agrandissement de l'école des Sentiers-Fleuris est rendu à environ 60 %. La coulée de la dalle du \
                 gymnase est prévue jeudi prochain, à 6 h 30. Une seule réserve : l'inspecteur veut voir le coupe-feu \
                 du local technique corrigé avant qu'on ferme les murs.",
                "07:52",
                "7 s · 0,04 $",
            ),
            ligne("toi", "As-tu des nouvelles du CPE pour la soumission ?", "08:09"),
            eve(
                "Oui, et c'est une bonne nouvelle : le CPE Les Mésangeais accepte notre soumission, environ 1,45 M$. \
                 Leur courriel est arrivé à 7 h 58. Veux-tu que je prépare le contrat pour ta signature ?",
                "08:09",
                "6 s · 0,03 $",
            ),
            ligne("toi", "Qui me doit le plus d'argent ?", "10:41"),
            eve(&dette, "10:41", "9 s · 0,05 $"),
            ligne(
                "alerte",
                "Un courriel du CPE vient d'arriver : Rosalie fait de la fièvre, il faudrait venir la chercher.",
                "13:31",
            ),
            ligne("toi", "La petite fait de la fièvre, je dois partir plus tôt.", "13:36"),
            eve(
                "Oh non, pauvre Rosalie. Va la chercher, je m'occupe de ton après-midi : ta rencontre de 15 h 30 avec \
                 l'architecte peut glisser à lundi, et Alexis peut mener la réunion de 16 h à l'entrepôt. Je les \
                 préviens ?",
                "13:36",
                "5 s · 0,03 $",
            ),
            ligne(
                "toi",
                "Rappelle-moi le hockey de Félix à 17 h. Il veut que je l'appelle « le Mur », maintenant.",
                "16:20",
            ),
            eve(
                &format!(
                    "C'est noté, je te le rappelle à 17 h. Bonne game au Mur ! Il embarque sur la glace à 17 h 30.\n\
                     ---\n```carte\n{rappel}\n```"
                ),
                "16:20",
                "4 s · 0,02 $",
            ),
        ]
    }
}

#[cfg(test)]
mod essais_de_la_vitrine {
    use super::*;

    fn vitrine() -> Vitrine {
        Vitrine { jour: crate::centre::numero_du_jour(JOUR_DE_LA_VITRINE).unwrap(), activite: None }
    }

    fn nombre(v: &Value) -> f64 {
        v.as_f64().unwrap()
    }

    #[test]
    fn ses_chiffres_se_tiennent() {
        let e = vitrine().entreprise(8 * 3600);
        let (k, h) = (&e["kpis"], &e["heures"]);
        let somme = |v: &Value, champ: &str| v.as_array().unwrap().iter().map(|x| nombre(&x[champ])).sum::<f64>();
        let pareils = |a: f64, b: f64| (a - b).abs() < 0.01;
        assert!(pareils(nombre(&k["en_retard"]), somme(&e["factures_en_retard"], "solde")), "le retard");
        assert_eq!(k["factures_en_retard"], e["factures_en_retard"].as_array().unwrap().len());
        let b = &e["balance_agee"];
        let tranches: f64 = ["a_venir", "0_30", "31_60", "61_90", "plus_de_90"].iter().map(|t| nombre(&b[t])).sum();
        assert!(pareils(tranches, nombre(&k["a_recevoir"])), "la balance âgée donne ce qui est à recevoir");
        let echu: f64 = ["0_30", "31_60", "61_90", "plus_de_90"].iter().map(|t| nombre(&b[t])).sum();
        assert!(pareils(echu, nombre(&k["en_retard"])), "ce qui est échu est en retard");
        assert_eq!(k["projets_en_cours"], e["projets"].as_array().unwrap().len());
        assert!(pareils(nombre(&k["valeur_projets_en_cours"]), somme(&e["projets"], "valeur")));
        assert!(pareils(nombre(&k["pipeline"]), somme(&e["pipeline"], "valeur")), "le pipeline et ses étapes");
        assert!(pareils(nombre(&h["cette_semaine"]), somme(&h["par_projet"], "heures")), "les heures des chantiers");
        // Une quarantaine d'employés, quatre jours d'environ huit heures.
        let par_employe = nombre(&h["cette_semaine"]) / nombre(&h["employes_actifs"]) / 4.0;
        assert!((7.5..=8.5).contains(&par_employe), "{par_employe} h par jour");
        // Les chantiers des factures, des commandes et des heures sont ceux de la carte.
        let chantiers: Vec<&str> =
            e["projets"].as_array().unwrap().iter().map(|p| p["nom"].as_str().unwrap()).collect();
        for liste in [&e["factures_recues"], &e["commandes"], &h["par_projet"]] {
            for x in liste.as_array().unwrap() {
                let nom = x.get("projet").or(x.get("nom")).and_then(Value::as_str).unwrap();
                assert!(chantiers.contains(&nom), "{nom} n'est pas un chantier en cours");
            }
        }
    }

    #[test]
    fn sa_conversation_dit_ce_que_montre_le_tableau() {
        let v = vitrine();
        let fil = v.conversation();
        let texte = |k: usize| fil[k]["texte"].as_str().unwrap().to_string();
        // La plus grosse facture en retard est la première de la carte : son client, son montant, ses jours.
        let (_, client, _, jours) = RETARDS_DE_LA_VITRINE[0];
        assert!(RETARDS_DE_LA_VITRINE.iter().all(|f| f.2 <= RETARDS_DE_LA_VITRINE[0].2));
        let dette = texte(5);
        assert!(dette.starts_with(client.trim_end_matches(" inc.")), "{dette}");
        assert!(dette.contains("142 000 $") && dette.contains(&format!("{jours} jours")), "{dette}");
        // Le chantier de l'école est sur la carte des chantiers, et sa coulée à l'agenda, jeudi prochain.
        let e = v.entreprise(8 * 3600);
        assert!(
            CHANTIERS_DE_LA_VITRINE.iter().any(|c| texte(1).contains(c.1.trim_start_matches("Agrandissement de ")))
        );
        let coulee = e["agenda"].as_array().unwrap().iter().find(|a| a["texte"].as_str().unwrap().contains("Coulée"));
        assert_eq!(crate::ambiance::date_longue(coulee.unwrap()["quand"].as_str().unwrap()).get(..5), Some("JEUDI"));
        // Le rappel du hockey : sa carte, et le même dans rappels.json.
        assert!(texte(10).contains(&format!("\"a\":\"{} 17:00\"", v.d(0))), "{}", texte(10));
        assert_eq!(v.rappels()[0]["a"], format!("{} 17:00", v.d(0)));
        // Chaque moment montre la conversation jusqu'à son heure, pas plus loin ; les moments se suivent.
        let seconde = |h: &str| seconde_du_jour(h).unwrap();
        for (k, m) in MOMENTS_DE_LA_VITRINE.iter().enumerate() {
            assert!(seconde(fil[m.lignes - 1]["heure"].as_str().unwrap()) <= m.heure, "{}", m.scene);
            if let Some(suivante) = fil.get(m.lignes) {
                assert!(seconde(suivante["heure"].as_str().unwrap()) > m.heure, "{}", m.scene);
            }
            if let Some(avant) = k.checked_sub(1).map(|j| &MOMENTS_DE_LA_VITRINE[j]) {
                assert!(avant.heure < m.heure && avant.lignes <= m.lignes, "{}", m.scene);
            }
        }
        assert_eq!(fil.len(), MOMENTS_DE_LA_VITRINE.last().unwrap().lignes);
    }
}

/// Ce que l'essai du thème publie à la fenêtre, comme le portrait du chef.
struct EtatDuTheme {
    theme: String,
    etat: &'static str,
    libelle: String,
    aide: String,
    niveau: f64,
    fil: Vec<Value>,
    accord: Value,
    /// Une phrase mal entendue, que la fenêtre met dans la saisie (le garde-fou de l'oreille).
    a_corriger: Value,
    /// Une réponse inventée est due à cet instant (un texte écrit dans la fenêtre).
    reponse_a: Option<Instant>,
    /// Eve dit sa réponse, de cet instant à celui-là (une voix inventée, que suit la bouche du portrait).
    parle: Option<(Instant, Instant)>,
    /// L'émotion de la réponse, que montre le visage d'Eve.
    emotion: Option<&'static str>,
    /// La vue du tableau de bord (le réglage `vue_du_tableau`) ; la liaison à Constructo AI, comme le chef la publie
    /// (`constructo`) ; l'`entreprise.json` où s'écrit l'entreprise inventée (`None` : rien ne s'écrit) ; une lecture
    /// (ou une liaison) inventée qui finit à cet instant.
    vue: String,
    constructo: Value,
    entreprise: Option<std::path::PathBuf>,
    lecture_finie_a: Option<Instant>,
    /// La vitrine : sa famille, son entreprise, sa journée ([`Vitrine`]) ; `None` : les données de l'essai.
    vitrine: Option<Vitrine>,
}

impl EtatDuTheme {
    fn nouveau(entreprise: Option<std::path::PathBuf>, vitrine: Option<Vitrine>) -> EtatDuTheme {
        EtatDuTheme {
            vitrine,
            theme: "constructo".into(),
            etat: "pret",
            libelle: String::new(),
            aide: String::new(),
            niveau: 0.0,
            fil: Vec::new(),
            accord: Value::Null,
            a_corriger: Value::Null,
            reponse_a: None,
            parle: None,
            emotion: None,
            vue: "tout".into(),
            constructo: Value::Null,
            entreprise,
            lecture_finie_a: None,
        }
    }

    /// Eve dit sa réponse pendant `duree`, comme le chef le publie (« Je réponds ») : une voix inventée, que suit sa
    /// bouche — des syllabes (quatre et demie par seconde), plus ou moins fortes, une pause toutes les 2,2 s, et les
    /// formes d'une vraie phrase posées dessus ([`crate::voix::piste_de`]).
    fn dire(&mut self, duree: Duration) {
        self.etat = "parole";
        self.libelle = "Je réponds".into();
        self.aide = "Dis « Eve, stop » — ou maintiens [Ctrl droit] — pour me couper".into();
        let maintenant = Instant::now();
        self.parle = Some((maintenant, maintenant + duree));
        let phrase = "Voici le budget de la sortie : pour le bus, les entrées et la collation, deux cent quarante \
                      dollars, sous les trois cents prévus. Bonne journée !";
        crate::voix::faire_entendre(Some(crate::voix::piste_de(voix_inventee(duree), phrase)));
    }

    /// Une ligne du fil.
    fn ligne(de: &str, texte: &str, heure: &str, ancien: bool) -> Value {
        serde_json::json!({ "de": de, "texte": texte, "heure": heure, "pieces": [], "ancien": ancien })
    }

    /// Eve est prête, et dit comment lui parler.
    fn prete(&mut self) {
        self.etat = "pret";
        self.libelle = "Prête".into();
        self.aide = "Dis « Eve… » — ou maintiens [Ctrl droit] pour parler".into();
        self.niveau = 0.0;
        self.parle = None;
        crate::voix::faire_entendre(None);
    }

    /// La liaison à Constructo AI, comme le chef la publie : son état (« connectee », « en_cours », « deconnectee ») et
    /// sa ligne.
    fn relier(&mut self, etat: &str) {
        let detail = match etat {
            "connectee" => "Reliée au compte sam@exemple.ca : ses outils sont prêts.",
            "en_cours" => {
                "La page de connexion de Constructo AI est ouverte dans ton navigateur : entre le courriel et le mot \
                 de passe de ton entreprise, puis reviens ici."
            }
            _ => "Pas encore reliée à Constructo AI.",
        };
        self.constructo = serde_json::json!({
            "permis": true,
            "etat": etat,
            "connectee": etat == "connectee",
            "branchee": etat == "connectee",
            "compte": if etat == "connectee" { "sam@exemple.ca" } else { "" },
            "erreur": Value::Null,
            "detail": detail,
        });
    }

    /// L'entreprise inventée, écrite comme Eve l'écrirait en lisant l'ERP (`None` : le fichier s'efface).
    fn ecrire_l_entreprise(&self, contenu: Option<Value>) {
        let Some(fichier) = &self.entreprise else { return };
        let fait = match contenu {
            Some(v) => crate::reglages::ecrire_d_un_geste(fichier, v.to_string().as_bytes()),
            None if fichier.exists() => std::fs::remove_file(fichier),
            None => Ok(()),
        };
        if let Err(e) = fait {
            eprintln!("{} : {e}", fichier.display());
        }
    }

    /// L'entreprise à jour, lue il y a `il_y_a` secondes : celle de la vitrine (à l'heure de son horloge), sinon celle
    /// de l'essai.
    fn entreprise_a_jour(&self, etat: &str, il_y_a: i64) -> Value {
        match &self.vitrine {
            Some(v) => {
                let maintenant = seconde_du_jour(&crate::journal::heure()).unwrap_or(12 * 3600);
                let mut e = v.entreprise((maintenant - il_y_a).max(0));
                e["etat"] = etat.into();
                e
            }
            None => entreprise_inventee(etat, il_y_a, None, true),
        }
    }

    /// Un moment de la journée de la vitrine : son heure (l'horloge de la fenêtre), son temps, ses courriels, la
    /// lecture de l'ERP faite quatre minutes plus tôt, le rappel du hockey une fois posé, la conversation jusque-là,
    /// et l'émotion d'Eve.
    fn moment_de_la_vitrine(&mut self, m: &MomentDeLaVitrine) {
        let Some(v) = &self.vitrine else { return };
        crate::journal::regler_l_horloge(Some(v.jour * 86_400 + m.heure));
        en_ligne_inventes(EnLigne::Aucun);
        if let Some(b) = v.bulletin(m.meteo) {
            crate::meteo::imposer(b);
        }
        if let Err(e) = v.ecrire_les_courriels(m.non_lus) {
            eprintln!("les courriels de la vitrine : {e}");
        }
        let entreprise = v.entreprise(m.heure - 4 * 60);
        let rappels = (m.lignes >= LIGNES_DU_RAPPEL).then(|| v.rappels());
        let fil: Vec<Value> = v.conversation().into_iter().take(m.lignes).collect();
        self.vue = m.vue.into();
        self.relier("connectee");
        self.ecrire_l_entreprise(Some(entreprise));
        self.ecrire_les_rappels(rappels);
        self.lecture_finie_a = None;
        self.fil = fil;
        self.emotion = m.emotion;
        self.accord = Value::Null;
        self.a_corriger = Value::Null;
        self.prete();
    }

    /// Les rappels de la vitrine dans `rappels.json` du dossier local de l'essai : écrits seulement s'il n'y en a pas
    /// ou s'ils viennent d'elle (sa marque à côté), effacés de même ; jamais ceux d'une vraie Eve.
    fn ecrire_les_rappels(&self, rappels: Option<Value>) {
        if self.entreprise.is_none() {
            return;
        }
        let local = crate::reglages::dossier_local();
        let (fichier, marque) = (local.join("rappels.json"), local.join("essai-theme-rappels.txt"));
        if fichier.exists() && !marque.is_file() {
            return;
        }
        let fait = match rappels {
            Some(r) => std::fs::write(&marque, "Ces rappels sont inventés par la vitrine de eve --essai-theme.\n")
                .and_then(|()| crate::reglages::ecrire_d_un_geste(&fichier, r.to_string().as_bytes())),
            None if fichier.exists() => std::fs::remove_file(&fichier),
            None => Ok(()),
        };
        if let Err(e) = fait {
            eprintln!("{} : {e}", fichier.display());
        }
    }

    /// L'état d'une scène (personne de vrai : une famille et une entreprise inventées). Ce qui se disait s'arrête.
    fn scene(&mut self, nom: &str) {
        self.parle = None;
        crate::voix::faire_entendre(None);
        // Un moment de la vitrine règle son horloge ; les autres scènes ont la vraie heure.
        if let Some(m) = MOMENTS_DE_LA_VITRINE.iter().find(|m| m.scene == nom) {
            self.moment_de_la_vitrine(m);
            return;
        }
        crate::journal::regler_l_horloge(None);
        // Les services en ligne et Stripe de l'essai, imposés dès la première scène : ceux des scènes des cartes en
        // ligne, l'ordinaire dans les autres.
        en_ligne_inventes(match nom {
            "en-ligne-incident" | "en-ligne-etroite" => EnLigne::Incident,
            "en-ligne-manquee" => EnLigne::Manquee,
            _ => EnLigne::Ordinaire,
        });
        // Le rappel qu'aurait laissé une vitrine d'avant ne sonne pas dans les autres scènes.
        self.ecrire_les_rappels(None);
        // L'émotion de chaque scène : la joie d'une réponse, l'empathie en la disant, le sérieux d'un accord.
        self.emotion = match nom {
            "reponse" => Some("joie"),
            "parole" => Some("empathie"),
            "accord" => Some("serieuse"),
            _ => None,
        };
        // La vue du tableau et l'entreprise de chaque scène ; les scènes de la conversation gardent celles d'avant.
        let lue = self.entreprise_a_jour("a_jour", 4 * 60);
        let a_jour = || Some(lue.clone());
        let panne = "le serveur de Constructo AI ne répond pas (il se réveille peut-être)";
        let entreprise = match nom {
            "tableau" | "retour" | "grande" | "suite" | "moyenne" => Some(("tout", "connectee", a_jour())),
            "entreprise" | "entreprise-suite" => Some(("entreprise", "connectee", a_jour())),
            "famille" => Some(("famille", "connectee", a_jour())),
            "non-reliee" => Some(("tout", "deconnectee", Some(entreprise_inventee("non_connecte", 0, None, false)))),
            "connexion" => Some(("tout", "en_cours", Some(entreprise_inventee("non_connecte", 0, None, false)))),
            "lecture" => Some(("tout", "connectee", Some(entreprise_inventee("en_cours", 0, None, false)))),
            "erreur" => {
                Some(("entreprise", "connectee", Some(entreprise_inventee("erreur", 3 * 3600, Some(panne), true))))
            }
            "erreur-sans-chiffres" => {
                Some(("tout", "connectee", Some(entreprise_inventee("erreur", 0, Some(panne), false))))
            }
            "perdue" => Some(("entreprise", "deconnectee", Some(entreprise_inventee("a_jour", 22 * 3600, None, true)))),
            // Les lectures manquées des cartes en ligne, pendant la première lecture de l'entreprise : ses cartes n'y
            // sont pas encore, celles en ligne se voient toutes en haut de la page.
            "en-ligne-manquee" => {
                Some(("entreprise", "connectee", Some(entreprise_inventee("en_cours", 0, None, false))))
            }
            n if n.starts_with("en-ligne") => Some(("entreprise", "connectee", a_jour())),
            _ => None,
        };
        if let Some((vue, liaison, contenu)) = entreprise {
            self.vue = vue.into();
            self.relier(liaison);
            self.ecrire_l_entreprise(contenu);
            self.lecture_finie_a = None;
        }
        match nom {
            "tableau" | "retour" => {
                self.prete();
                self.accord = Value::Null;
                if self.fil.is_empty() {
                    self.fil = vec![
                        Self::ligne("toi", "Qu'est-ce qu'on mange ce soir ?", "17:42", true),
                        Self::ligne(
                            "eve",
                            "Une **lasagne aux légumes** : la sauce tomate est à sortir du congélateur.",
                            "17:42",
                            true,
                        ),
                        Self::ligne("systeme", "— Nouvelle conversation —", "", false),
                        Self::ligne("toi", "Quelles sont les échéances de la semaine ?", "09:14", false),
                        Self::ligne(
                            "eve",
                            "Trois échéances arrivent :\n- **Journée pédagogique** demain : pas d'école\n- \
                             L'**acompte provisionnel**, dans 6 jours\n- Le rendez-vous chez l'optométriste, dans 12 \
                             jours\n---\nJe les ai lues dans `activite/echeances.js` ; le calendrier scolaire est sur \
                             [quebec.ca](https://www.quebec.ca/education).",
                            "09:14",
                            false,
                        ),
                        Self::ligne(
                            "alerte",
                            "Un courriel de l'école vient d'arriver : la sortie au verger est reportée à jeudi.",
                            "09:20",
                            false,
                        ),
                    ];
                }
            }
            "reflexion" => {
                self.fil.push(Self::ligne("toi", "Fais-moi le budget de la sortie au verger.", "09:21", false));
                self.etat = "reflexion";
                self.libelle = "Je m'en occupe…".into();
                self.aide = "Dis « Eve, arrête » pour m'interrompre".into();
            }
            "reponse" => {
                self.fil.push(Self::ligne(
                    "eve",
                    "Voici le budget de la sortie :\n\n| Poste | Montant |\n|---|---:|\n| Autobus | 120,00 $ |\n| \
                     Entrées au verger | 84,50 $ |\n| Collation | 36,00 $ |\n\nEn tout : **240,50 $**, sous les 300 $ \
                     prévus.",
                    "09:21",
                    false,
                ));
                // Ce que le tour a pris, comme le chef le publie sous une réponse de Claude.
                if let Some(l) = self.fil.last_mut() {
                    l["tour"] = Value::String("9 s · 0,03 $ · Sonnet 5.5, moyen".into());
                }
                self.prete();
            }
            "parole" => self.dire(Duration::from_secs(5)),
            "accord" => {
                self.fil.push(Self::ligne("toi", "Efface les vieux brouillons.", "09:23", false));
                self.etat = "confirmation";
                self.libelle = "J'attends ton accord".into();
                self.aide = "Dis « je confirme » ou « annule »".into();
                self.accord = serde_json::json!({
                    "numero": 1,
                    "quoi": "Supprimer 3 brouillons",
                    "detail": "Brouillons\\vieux-menu.md\nBrouillons\\liste-2025.md\nBrouillons\\notes.txt",
                    "restants": 1,
                });
            }
            "etroite" => {
                self.accord = Value::Null;
                self.fil.push(Self::ligne("systeme", "Les brouillons sont supprimés.", "", false));
                // Ce que dirait le garde-fou de l'oreille d'une boucle, et la phrase repliée qu'il met dans la saisie.
                let mal = "Mal entendu (une boucle de l'oreille) : rien n'est parti. La phrase est dans la saisie.";
                self.fil.push(Self::ligne("systeme", mal, "", false));
                self.a_corriger = serde_json::json!({ "numero": 1, "texte": "Ajoute du lait à l'épi" });
                self.etat = "ecoute";
                self.libelle = "Je t'écoute".into();
                self.aide = "Relâche pour envoyer".into();
                self.niveau = 0.45;
            }
            _ => self.prete(),
        }
    }

    /// Un geste de la fenêtre, comme le chef le prendrait ; faux pour « quitter ».
    fn geste(&mut self, action: &str, valeur: &Value) -> bool {
        println!("geste : {action} {valeur}");
        match (action, valeur.as_str()) {
            ("quitter", _) => return false,
            ("theme", Some(t)) => self.theme = t.to_string(),
            ("vue_du_tableau", Some(v)) if crate::reglages::VUES_DU_TABLEAU.iter().any(|x| x.0 == v) => {
                self.vue = v.to_string();
            }
            // Relire : la lecture d'avant reste, son état dit qu'Eve lit ; deux secondes plus tard, elle est à jour.
            ("entreprise_lire", _) if self.constructo["etat"].as_str() == Some("connectee") => {
                self.ecrire_l_entreprise(Some(self.entreprise_a_jour("en_cours", 4 * 60)));
                self.lecture_finie_a = Some(Instant::now() + Duration::from_secs(2));
            }
            // Se relier : la page de connexion s'ouvre, puis la liaison se fait en deux secondes et demie.
            ("constructo_connecter", _) => {
                self.relier("en_cours");
                self.lecture_finie_a = Some(Instant::now() + Duration::from_millis(2500));
            }
            ("texte", Some(t)) => {
                self.fil.push(Self::ligne("toi", t, &crate::journal::heure(), false));
                self.etat = "reflexion";
                self.libelle = "Je m'en occupe…".into();
                self.reponse_a = Some(Instant::now() + Duration::from_millis(1500));
            }
            ("nouvelle", _) => {
                let nouvelle = "— Nouvelle conversation : la mémoire repart de zéro —";
                self.fil = vec![Self::ligne("systeme", nouvelle, "", false)];
            }
            ("parler_debut", _) => {
                self.etat = "ecoute";
                self.libelle = "Je t'écoute".into();
            }
            ("parler_fin" | "arreter", _) => self.prete(),
            ("confirmer" | "annuler", _) => {
                let dit =
                    if action == "confirmer" { "Les brouillons sont supprimés." } else { "Rien n'est supprimé." };
                self.accord = Value::Null;
                self.fil.push(Self::ligne("systeme", dit, "", false));
                self.prete();
            }
            _ => {}
        }
        true
    }

    /// La réponse inventée à un texte, quand elle est due, puis dite trois secondes et demie ; la fin de ce qui se
    /// dit ; la fin d'une lecture ou d'une liaison inventée (reliée, à jour).
    fn avancer(&mut self) {
        if self.reponse_a.is_some_and(|t| Instant::now() >= t) {
            self.reponse_a = None;
            let texte = "Ceci est un **essai** : je n'ai rien fait, mais voilà où ma réponse s'écrirait.";
            self.fil.push(Self::ligne("eve", texte, &crate::journal::heure(), false));
            self.emotion = Some("amusee");
            self.dire(Duration::from_millis(3500));
        }
        if self.parle.is_some_and(|(_, fin)| Instant::now() >= fin) {
            self.prete();
        }
        if self.lecture_finie_a.is_some_and(|t| Instant::now() >= t) {
            self.lecture_finie_a = None;
            self.relier("connectee");
            self.ecrire_l_entreprise(Some(self.entreprise_a_jour("a_jour", 0)));
        }
    }

    fn json(&self) -> String {
        serde_json::json!({
            "etat": self.etat,
            "libelle": self.libelle,
            "aide": self.aide,
            "niveau": self.niveau,
            "emotion": self.emotion,
            "qui": "Sam",
            "theme": self.theme,
            "vue_du_tableau": self.vue,
            // La vitrine se montre comme une vraie Eve : le profil du bureau, un micro.
            "nom": if self.vitrine.is_some() { "Bureau" } else { "Essai" },
            "propre": true,
            "entreprise": "",
            "base": "",
            "micro": if self.vitrine.is_some() { "Microphone" } else { "Le micro de l'essai" },
            "oreille": "prete",
            "accord": self.accord,
            "a_corriger": self.a_corriger,
            "activite": [],
            "outils": [],
            "pieces": [],
            "fil_rev": self.fil.len(),
            "fil": self.fil,
            "en_cours": Value::Null,
            "voix": { "liste": [{ "id": "essai", "nom": "La voix de l'essai" }], "choisie": "essai" },
            "reglages": {
                "eveil": true,
                "interruption": true,
                "sourdine": true,
                "alertes": { "suspects": false, "factures": false, "pieces": false, "rendez_vous": false },
                "devant": false,
                "demarrage": false,
                "historique": false,
                "raccourci": "Ctrl+Alt+E",
            },
            "eveil": true,
            "touche": "Ctrl droit",
            "camera": false,
            "camera_ordre": { "n": 0, "geste": "" },
            "constructo": self.constructo,
        })
        .to_string()
    }
}

/// Une voix inventée de `duree` : son niveau toutes les 40 ms — quatre syllabes et demie par seconde, plus ou moins
/// fortes, une pause toutes les 2,2 s.
fn voix_inventee(duree: Duration) -> Arc<[f32]> {
    let pas = (duree.as_secs_f64() / 0.04) as usize;
    (0..pas)
        .map(|k| {
            let t = k as f64 * 0.04;
            let syllabe = (t * 4.5 * std::f64::consts::TAU).sin().max(0.0);
            let force = 0.6 + 0.4 * (t * 1.3).sin();
            let pause = if t % 2.2 > 1.9 { 0.0 } else { 1.0 };
            (syllabe * force * pause) as f32
        })
        .collect()
}

/// Le chef de l'essai du thème : il fait défiler les scènes (la taille de la fenêtre, une capture à la fin de chacune),
/// prend les gestes de la fenêtre, et lui publie l'état quand il change.
fn mener_l_essai_du_theme(
    ecran: &crate::ecran::Ecran,
    gestes: &mpsc::Receiver<Evt>,
    mut etat: EtatDuTheme,
    scenes: &[&SceneDuTheme],
    captures: Option<std::path::PathBuf>,
    secondes: Option<f64>,
) {
    let debut = Instant::now();
    let (mut scene, mut debut_de_scene, mut capturee) = (0usize, None::<f64>, false);
    let mut publie = String::new();
    loop {
        let t = debut.elapsed().as_secs_f64();
        if secondes.is_some_and(|s| t >= s) {
            crate::natif::fermer();
            return;
        }
        match gestes.recv_timeout(Duration::from_millis(50)) {
            Ok(Evt::Ui { action, valeur }) if !etat.geste(&action, &valeur) => {
                crate::natif::fermer();
                return;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            _ => {}
        }
        etat.avancer();
        // Les scènes attendent la fenêtre : son contexte est là dès sa première image.
        if let Some(ctx) = crate::natif::theme::contexte()
            && let Some(s) = scenes.get(scene)
        {
            let depuis = *debut_de_scene.get_or_insert_with(|| {
                println!("scène : {}", s.nom);
                if scene > 0 {
                    etat.scene(s.nom);
                }
                if let Some([l, h]) = s.taille {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(l, h)));
                }
                // Les autres scènes montrent le haut de la page, même après une scène défilée.
                if !s.nom.ends_with("suite") {
                    crate::natif::theme::defiler_le_tableau(&ctx, 0.0);
                }
                t
            });
            // Le bas de la page, jusqu'à une demi-seconde de la capture : la vue et la taille de la scène se prennent
            // entre-temps. Celui des cartes en ligne, tout en bas : egui ramène un défilement trop long au bout de la
            // page à la fin d'une image, et l'image suivante s'y dessine — encore faut-il ne plus le redemander.
            if s.nom.ends_with("suite") && t - depuis < s.duree - 1.5 {
                let y = if s.nom.starts_with("en-ligne") { 10_000.0 } else { 820.0 };
                crate::natif::theme::defiler_le_tableau(&ctx, y);
            }
            if let Some(dossier) = &captures
                && !capturee
                && t - depuis >= s.duree - 1.0
            {
                capturee = true;
                // Le rang de la scène dans l'essai entier : une capture garde son nom d'un essai à l'autre ; celles de
                // la vitrine portent le nom de leur moment (« vitrine-joie.png »).
                let rang = SCENES_DU_THEME.iter().position(|x| x.nom == s.nom).unwrap_or(scene) + 1;
                let nom = if s.nom.starts_with("vitrine") {
                    format!("{}.png", s.nom)
                } else {
                    format!("theme-{rang:02}-{}.png", s.nom)
                };
                let chemin = dossier.join(nom);
                ctx.request_screenshot(move |image| match enregistrer_png(&image, &chemin) {
                    Ok(()) => println!("capture : {}", chemin.display()),
                    Err(e) => eprintln!("capture {} : {e}", chemin.display()),
                });
            }
            if t - depuis >= s.duree {
                (scene, debut_de_scene, capturee) = (scene + 1, None, false);
                if scene == scenes.len() && captures.is_some() {
                    thread::sleep(Duration::from_millis(600));
                    crate::natif::fermer();
                    return;
                }
            }
        }
        let json = etat.json();
        if json != publie {
            ecran.publier(&json);
            publie = json;
        }
    }
}

// ---- Les pièces jointes de la fenêtre native (le lot 6.3) ---------------------------------------------------------

/// Les fichiers inventés de l'essai des pièces et leur taille, dans le dossier `essai-pieces` du dossier local ; avec
/// eux, un dossier, `DOSSIER_DES_PIECES`.
const FICHIERS_DES_PIECES: [(&str, u64); 6] = [
    ("Facture Matériaux Exemple 9013623395.pdf", 16_931),
    ("photo-balcon.jpg", 2_457_600),
    ("Le plan du sous-sol, avec les cotes de la salle de jeu, de l'atelier et de la chambre d'amis.pdf", 487_210),
    ("devis-fenetres.pdf", 98_304),
    ("vide.txt", 0),
    ("trop-gros.bin", 41 * 1024 * 1024),
];
const DOSSIER_DES_PIECES: &str = "un dossier";

/// `eve --essai-pieces [--captures DOSSIER] [--sans-selecteur]` : la vraie fenêtre native, sur un état inventé que
/// l'essai lui publie comme le chef le ferait (il tient ses pièces en attente, il prend ses gestes), et ses pièces
/// jointes mises à l'épreuve sans la souris de la personne : des fichiers inventés glissés sur la fenêtre (le voile),
/// déposés (leurs pastilles), un fichier de plus de 40 Mo, un fichier vide et un dossier (leurs refus, au fil), le ×
/// d'une pastille, une demande qui emporte les pièces ; le centre de commande ; puis le thème « Constructo AI », et le
/// trombone : sa boîte de dialogue, que l'essai remplit et valide (une boîte de ce programme, attachée à sa fenêtre, et
/// aucune autre ; `--sans-selecteur` s'en passe). Enfin la page : un fichier envoyé comme elle l'envoie
/// (`POST /piece`). Le bilan dit ce qui va. Tout se joint par la vraie fonction (`ecran::joindre`), dans le dossier
/// local : l'essai veut le sien (`EVE_DOSSIER_LOCAL`), jamais celui d'une vraie Eve ; ses fichiers inventés y sont
/// aussi. `--captures` : la fenêtre, par elle-même, en PNG, à chaque étape. La musique du centre se tait (A, S) avant
/// de jouer.
pub fn pieces(args: &[String]) -> i32 {
    let captures =
        args.iter().position(|a| a == "--captures").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    let selecteur = !args.iter().any(|a| a == "--sans-selecteur");
    if std::env::var_os("EVE_DOSSIER_LOCAL").is_none_or(|d| d.is_empty()) {
        eprintln!(
            "eve --essai-pieces joint ses fichiers dans le dossier local : nomme-lui le sien (EVE_DOSSIER_LOCAL)."
        );
        return 2;
    }
    let source = crate::reglages::dossier_local().join("essai-pieces");
    let ecrits = std::fs::create_dir_all(source.join(DOSSIER_DES_PIECES)).and_then(|()| {
        // Un contenu quelconque, à la bonne taille : la fenêtre n'en lit rien ; le gros se fait d'un coup.
        FICHIERS_DES_PIECES
            .iter()
            .try_for_each(|(nom, taille)| std::fs::File::create(source.join(nom))?.set_len(*taille))
    });
    if let Err(e) = ecrits {
        eprintln!("{} : {e}", source.display());
        return 1;
    }
    if let Some(c) = &captures
        && let Err(e) = std::fs::create_dir_all(c)
    {
        eprintln!("{} : {e}", c.display());
        return 1;
    }
    println!("les fichiers inventés : {}", source.display());
    // La vitrine de l'essai, sur un port libre (la page doit le connaître) : la fenêtre s'y abonne comme à celle
    // d'Eve, la page y envoie sa pièce.
    let (rapport, recus) = mpsc::channel::<Evt>();
    let port = std::net::TcpListener::bind(("127.0.0.1", 0)).and_then(|l| l.local_addr()).map_or(0, |a| a.port());
    let ecran = match crate::ecran::demarrer(port, crate::ecran::jeton_aleatoire(), rapport.clone()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("la vitrine de l'essai ne s'ouvre pas : {e}");
            return 1;
        }
    };
    let chef = Arc::new(Mutex::new(ChefDesPieces::nouveau()));
    ecran.publier(&chef.lock().map(|c| c.json()).unwrap_or_default());
    let abonnement = ecran.abonner();
    let fil_du_chef = {
        let (ecran, chef) = (ecran.clone(), chef.clone());
        thread::spawn(move || mener_le_chef_des_pieces(&ecran, &recus, &chef))
    };
    let titre = format!("{} · Constructo AI", crate::fenetre::titre_pour("Essai des pièces"));
    let vue = egui::ViewportBuilder::default()
        .with_title(&titre)
        .with_inner_size([900.0, 860.0])
        .with_min_inner_size([400.0, 540.0])
        .with_active(false);
    let options = eframe::NativeOptions { viewport: vue, ..Default::default() };
    let problemes = Arc::new(Mutex::new(Vec::<String>::new()));
    let (chef_de_l_essai, source_de_l_essai, problemes_de_l_essai) = (chef.clone(), source.clone(), problemes.clone());
    let creer: eframe::AppCreator<'_> = Box::new(move |cc| {
        Ok(Box::new(EssaiDesPieces {
            fenetre: crate::natif::Fenetre::nouvelle(cc, abonnement, rapport, None),
            chef: chef_de_l_essai,
            source: source_de_l_essai,
            parcours: parcours_des_pieces(selecteur),
            debut: Instant::now(),
            prochain: 0.0,
            attente: None,
            pointeur: egui::Pos2::ZERO,
            survoles: Vec::new(),
            captures,
            a_capturer: Vec::new(),
            selecteur: None,
            problemes: problemes_de_l_essai,
        }))
    });
    if let Err(e) = eframe::run_native(&titre, options, creer) {
        eprintln!("la fenêtre ne s'ouvre pas : {e}");
        return 1;
    }
    // La page, la fenêtre fermée : une pièce et une pièce vide, envoyées comme elle les envoie.
    let page = envoyer_comme_la_page(ecran.port, &ecran.jeton, "depuis-la-page.txt", b"Une piece, comme la page.");
    let page_vide = envoyer_comme_la_page(ecran.port, &ecran.jeton, "vide-par-la-page.txt", b"");
    thread::sleep(Duration::from_millis(500));
    if let Ok(mut c) = chef.lock() {
        c.fini = true;
    }
    let _ = fil_du_chef.join();
    let problemes = problemes.lock().map(|p| p.clone()).unwrap_or_default();
    let Ok(c) = chef.lock() else { return 1 };
    bilan_des_pieces(&c, &source, &problemes, selecteur, (&page, &page_vide))
}

/// Un pas du parcours de l'essai des pièces.
enum PasDesPieces {
    /// Le pas suivant attend ce temps-là, en secondes.
    Pause(f64),
    /// Ces fichiers inventés se glissent sur la fenêtre (le voile), jusqu'à ce qu'ils s'y déposent.
    Survoler(&'static [&'static str]),
    Deposer(&'static [&'static str]),
    /// Une image collée (Ctrl+V après une capture d'écran), comme egui la donne : un petit rectangle bleu.
    Coller,
    /// Le chef de l'essai tient ce nombre de pièces (le parcours l'attend six secondes au plus).
    Pieces(usize),
    /// Le pointeur sur la cible ; son bouton qui s'enfonce, qui se relâche.
    Survol(CibleDesPieces),
    Appuyer,
    Relacher,
    /// Du texte dans le champ qui a le clavier, puis Entrée.
    Taper(&'static str),
    /// Des touches du clavier, enfoncées puis relâchées (hors de la saisie : celles du centre de commande).
    Touches(&'static [egui::Key]),
    /// Ce que l'essai publie : l'état d'Eve, le thème.
    Etat(&'static str),
    Theme(&'static str),
    Taille(f32, f32),
    /// La boîte de dialogue que le trombone vient d'ouvrir : l'essai y choisit ces fichiers, dans un fil ; puis le
    /// parcours attend ce fil (vingt secondes au plus).
    Selecteur(&'static [&'static str]),
    FinDuSelecteur,
    /// Une capture de l'image en cours ; le pas suivant attend qu'elle soit prise.
    Capture(&'static str),
    Fin,
}

/// Ce que le pointeur de l'essai vise : le trombone, le × d'une pièce (son numéro), le champ de la saisie.
#[derive(Debug)]
enum CibleDesPieces {
    Trombone,
    Retirer(&'static str),
    Saisie,
}

/// Le parcours de l'essai : le thème sombre, la conversation seule (le voile, les pastilles, les refus, le ×, la
/// demande), puis le centre de commande ; le thème « Constructo AI » (le voile, les pastilles, le trombone et sa boîte
/// de dialogue), puis sa conversation seule.
fn parcours_des_pieces(selecteur: bool) -> VecDeque<PasDesPieces> {
    use CibleDesPieces::{Retirer, Saisie, Trombone};
    use PasDesPieces::*;
    const DEPOSEES: [&str; 3] = [FICHIERS_DES_PIECES[0].0, FICHIERS_DES_PIECES[1].0, FICHIERS_DES_PIECES[2].0];
    const REFUSEES: [&str; 3] = [FICHIERS_DES_PIECES[5].0, FICHIERS_DES_PIECES[4].0, DOSSIER_DES_PIECES];
    const AU_CENTRE: [&str; 1] = [FICHIERS_DES_PIECES[3].0];
    const CLAIRES: [&str; 2] = [FICHIERS_DES_PIECES[3].0, FICHIERS_DES_PIECES[1].0];
    const CHOISIES: [&str; 2] = [FICHIERS_DES_PIECES[0].0, FICHIERS_DES_PIECES[2].0];
    let mut v = vec![
        Touches(&[egui::Key::A, egui::Key::S]),
        Pause(0.8),
        Etat("pret"),
        Pause(1.2),
        Survoler(&DEPOSEES),
        Pause(1.0),
        Capture("01-sombre-voile"),
        Pause(0.3),
        Deposer(&DEPOSEES),
        Pieces(3),
        Pause(0.8),
        Capture("02-sombre-pastilles"),
        Deposer(&REFUSEES),
        Pause(1.2),
        Capture("03-sombre-refus"),
        Survol(Retirer("p2")),
        Pause(0.6),
        Capture("04-sombre-retirer"),
        Appuyer,
        Relacher,
        Pieces(2),
        Pause(0.4),
        Survol(Saisie),
        Appuyer,
        Relacher,
        Pause(0.2),
        Taper("Classe ces pièces dans le bon dossier."),
        Pieces(0),
        Pause(0.8),
        Capture("05-sombre-envoyees"),
        Taille(1500.0, 900.0),
        Pause(1.2),
        Deposer(&AU_CENTRE),
        Pieces(1),
        Pause(1.0),
        Capture("06-sombre-centre"),
        Theme("constructo"),
        Pause(1.6),
        Survoler(&CLAIRES),
        Pause(1.0),
        Capture("07-clair-voile"),
        Pause(0.3),
        Deposer(&CLAIRES),
        Pieces(3),
        Pause(0.8),
        Capture("08-clair-pastilles"),
    ];
    if selecteur {
        v.extend([
            Survol(Trombone),
            Pause(0.6),
            Capture("09-clair-trombone"),
            Appuyer,
            Relacher,
            Selecteur(&CHOISIES),
            FinDuSelecteur,
            Pieces(5),
            Pause(0.8),
            Capture("10-clair-choisies"),
        ]);
    }
    v.extend([
        Taille(980.0, 820.0),
        Pause(1.5),
        Capture("11-clair-etroite"),
        Coller,
        Pieces(if selecteur { 6 } else { 4 }),
        Pause(0.8),
        Capture("12-clair-collee"),
        Fin,
    ]);
    v.into()
}

/// Un fichier que l'essai dépose sur la fenêtre, comme Windows le donnerait à egui.
#[derive(Debug)]
struct FichierDepose(std::path::PathBuf);

impl egui::DroppedFile for FichierDepose {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Ce que le chef de l'essai a reçu : le bilan le relit.
#[derive(Debug, PartialEq)]
enum RecuDesPieces {
    Piece {
        nom: String,
        chemin: std::path::PathBuf,
        taille: u64,
    },
    Refus(String),
    /// Une pièce retirée (son numéro), et son fichier effacé ou non.
    Retrait {
        id: String,
        efface: bool,
    },
    /// Une demande écrite, et les pièces qui partent avec elle.
    Demande(String, Vec<String>),
    Autre(String),
}

/// Le chef de l'essai des pièces : il tient les pièces en attente (leur numéro, leur nom, leur chemin, leur taille)
/// comme le vrai, et le fil ; il publie son état à la fenêtre.
struct ChefDesPieces {
    etat: &'static str,
    theme: &'static str,
    pieces: Vec<(String, String, std::path::PathBuf, u64)>,
    compteur: u64,
    fil: Vec<Value>,
    recus: Vec<RecuDesPieces>,
    fini: bool,
}

impl ChefDesPieces {
    fn nouveau() -> ChefDesPieces {
        let bonjour = "Glisse un fichier sur la fenêtre, ou clique le trombone : il part avec ta prochaine demande.";
        ChefDesPieces {
            etat: "ecoute",
            theme: "centre",
            pieces: Vec::new(),
            compteur: 0,
            fil: vec![Self::ligne("eve", bonjour, "09:30", &[])],
            recus: Vec::new(),
            fini: false,
        }
    }

    fn ligne(de: &str, texte: &str, heure: &str, pieces: &[String]) -> Value {
        serde_json::json!({ "de": de, "texte": texte, "heure": heure, "pieces": pieces, "ancien": false })
    }

    /// Ce que la fenêtre envoie, pris comme le chef le prend (chef.rs : `joindre`, `piece_retirer`, `piece_erreur`,
    /// `phrase`).
    fn recevoir(&mut self, evt: Evt) {
        match evt {
            Evt::Piece { nom, chemin, taille } => {
                println!("pièce : {nom} ({taille} octets) → {}", chemin.display());
                self.compteur += 1;
                self.pieces.push((format!("p{}", self.compteur), nom.clone(), chemin.clone(), taille));
                self.recus.push(RecuDesPieces::Piece { nom, chemin, taille });
            }
            Evt::Ui { action, valeur } => {
                println!("geste : {action} {valeur}");
                match (action.as_str(), valeur.as_str()) {
                    ("piece_retirer", Some(id)) => {
                        let i = self.pieces.iter().position(|p| p.0 == id);
                        let efface = i.is_some_and(|i| std::fs::remove_file(&self.pieces.remove(i).2).is_ok());
                        self.recus.push(RecuDesPieces::Retrait { id: id.to_string(), efface });
                    }
                    ("piece_erreur", Some(e)) => {
                        self.fil.push(Self::ligne("systeme", e, "", &[]));
                        self.recus.push(RecuDesPieces::Refus(e.to_string()));
                    }
                    ("texte", Some(t)) => {
                        let noms: Vec<String> = self.pieces.drain(..).map(|p| p.1).collect();
                        let heure = crate::journal::heure();
                        self.fil.push(Self::ligne("toi", t, &heure, &noms));
                        let reponse = format!("Ceci est un **essai** : j'aurais lu tes {} pièces.", noms.len());
                        self.fil.push(Self::ligne("eve", &reponse, &heure, &[]));
                        self.recus.push(RecuDesPieces::Demande(t.to_string(), noms));
                    }
                    _ => self.recus.push(RecuDesPieces::Autre(action)),
                }
            }
            _ => {}
        }
    }

    fn json(&self) -> String {
        let (libelle, aide) = match self.etat {
            "ecoute" => ("Je t'écoute", "Relâche pour envoyer"),
            _ => ("Prête", "Dis « Eve… » — ou maintiens [Ctrl droit] pour parler"),
        };
        let pieces: Vec<Value> = self
            .pieces
            .iter()
            .map(|(id, nom, _, taille)| serde_json::json!({ "id": id, "nom": nom, "taille": taille }))
            .collect();
        serde_json::json!({
            "etat": self.etat,
            "libelle": libelle,
            "aide": aide,
            "niveau": 0.0,
            "qui": "Sam",
            "theme": self.theme,
            "nom": "Essai des pièces",
            "propre": true,
            "entreprise": "",
            "base": "",
            "micro": "Le micro de l'essai",
            "oreille": "prete",
            "accord": Value::Null,
            "activite": [],
            "outils": [],
            "pieces": pieces,
            "fil_rev": self.fil.len(),
            "fil": self.fil,
            "en_cours": Value::Null,
            "voix": { "liste": [{ "id": "essai", "nom": "La voix de l'essai" }], "choisie": "essai" },
            "reglages": {
                "eveil": true,
                "interruption": true,
                "sourdine": true,
                "alertes": { "suspects": false, "factures": false, "pieces": false, "rendez_vous": false },
                "devant": false,
                "demarrage": false,
                "historique": false,
                "raccourci": "Ctrl+Alt+E",
            },
            "eveil": true,
            "touche": "Ctrl droit",
            "camera": false,
            "camera_ordre": { "n": 0, "geste": "" },
        })
        .to_string()
    }
}

/// Le fil du chef de l'essai : il prend ce que la fenêtre et la vitrine lui envoient, et publie son état quand il
/// change, jusqu'à la fin de l'essai.
fn mener_le_chef_des_pieces(ecran: &crate::ecran::Ecran, recus: &mpsc::Receiver<Evt>, chef: &Mutex<ChefDesPieces>) {
    let mut publie = String::new();
    loop {
        match recus.recv_timeout(Duration::from_millis(40)) {
            Ok(evt) => {
                if let Ok(mut c) = chef.lock() {
                    c.recevoir(evt);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let json = match chef.lock() {
            Ok(c) if !c.fini => c.json(),
            _ => return,
        };
        if json != publie {
            ecran.publier(&json);
            publie = json;
        }
    }
}

/// La fenêtre de l'essai : la vraie fenêtre d'Eve, et le parcours qui lui donne ses gestes.
struct EssaiDesPieces {
    fenetre: crate::natif::Fenetre,
    chef: Arc<Mutex<ChefDesPieces>>,
    /// Le dossier des fichiers inventés.
    source: std::path::PathBuf,
    /// Le parcours, son départ, l'heure de son prochain pas (en secondes), depuis quand il attend des pièces ; le
    /// pointeur qu'il promène ; les fichiers qu'il glisse sur la fenêtre.
    parcours: VecDeque<PasDesPieces>,
    debut: Instant,
    prochain: f64,
    attente: Option<f64>,
    pointeur: egui::Pos2,
    survoles: Vec<std::path::PathBuf>,
    captures: Option<std::path::PathBuf>,
    a_capturer: Vec<&'static str>,
    /// Le fil qui remplit la boîte de dialogue ; ce qui n'a pas marché, pour le bilan.
    selecteur: Option<thread::JoinHandle<Result<(), String>>>,
    problemes: Arc<Mutex<Vec<String>>>,
}

impl EssaiDesPieces {
    fn position(ctx: &egui::Context, cible: &CibleDesPieces) -> Option<egui::Pos2> {
        let id = match cible {
            CibleDesPieces::Trombone => crate::natif::pieces::id_du_trombone(),
            CibleDesPieces::Retirer(id) => crate::natif::pieces::id_du_retrait(id),
            // Le champ de la saisie : celui du thème sombre, sinon celui du thème clair.
            CibleDesPieces::Saisie => {
                let sombre = egui::Id::unique("saisie-texte");
                if ctx.read_response(sombre).is_some() { sombre } else { egui::Id::unique("saisie-texte-constructo") }
            }
        };
        ctx.read_response(id).map(|r| r.rect.center())
    }

    fn probleme(&self, quoi: String) {
        eprintln!("essai : {quoi}");
        if let Ok(mut p) = self.problemes.lock() {
            p.push(quoi);
        }
    }

    /// Les pas venus à leur heure, en événements d'egui ; un pas qui attend des pièces retient la suite.
    fn avancer(&mut self, ctx: &egui::Context, entree: &mut egui::RawInput) {
        let maintenant = self.debut.elapsed().as_secs_f64();
        let rien = egui::Modifiers::NONE;
        let touche =
            |key, pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: rien };
        while maintenant >= self.prochain {
            let Some(pas) = self.parcours.pop_front() else { return };
            let pause = match pas {
                PasDesPieces::Pause(s) => s,
                PasDesPieces::Survoler(noms) => {
                    self.survoles = noms.iter().map(|n| self.source.join(n)).collect();
                    0.0
                }
                PasDesPieces::Deposer(noms) => {
                    self.survoles.clear();
                    entree.hovered_files.clear();
                    let source = &self.source;
                    entree.dropped_files.extend(
                        noms.iter().map(|n| Arc::new(FichierDepose(source.join(n))) as egui::DroppedFileHandle),
                    );
                    0.0
                }
                PasDesPieces::Coller => {
                    let image = egui::ColorImage::filled([64, 48], egui::Color32::from_rgb(0x25, 0x63, 0xEB));
                    entree.events.push(egui::Event::PasteImage(Arc::new(image)));
                    0.0
                }
                PasDesPieces::Pieces(n) => {
                    let tenues = self.chef.lock().map_or(n, |c| c.pieces.len());
                    if tenues != n {
                        let depuis = *self.attente.get_or_insert(maintenant);
                        if maintenant - depuis < 6.0 {
                            self.parcours.push_front(PasDesPieces::Pieces(n));
                            return;
                        }
                        self.probleme(format!("{n} pièce(s) attendue(s) au chef, il en tient {tenues}"));
                    }
                    self.attente = None;
                    0.0
                }
                PasDesPieces::Survol(cible) => match Self::position(ctx, &cible) {
                    Some(p) => {
                        self.pointeur = p;
                        entree.events.push(egui::Event::PointerMoved(p));
                        0.15
                    }
                    None => {
                        self.probleme(format!("{cible:?} introuvable"));
                        0.0
                    }
                },
                PasDesPieces::Appuyer | PasDesPieces::Relacher => {
                    entree.events.push(egui::Event::PointerButton {
                        pos: self.pointeur,
                        button: egui::PointerButton::Primary,
                        pressed: matches!(pas, PasDesPieces::Appuyer),
                        modifiers: rien,
                    });
                    0.12
                }
                PasDesPieces::Taper(texte) => {
                    entree.events.push(egui::Event::Text(texte.to_string()));
                    entree.events.extend([touche(egui::Key::Enter, true), touche(egui::Key::Enter, false)]);
                    0.2
                }
                PasDesPieces::Touches(touches) => {
                    entree.events.extend(touches.iter().flat_map(|&k| [touche(k, true), touche(k, false)]));
                    0.1
                }
                PasDesPieces::Etat(e) => {
                    if let Ok(mut c) = self.chef.lock() {
                        c.etat = e;
                    }
                    0.0
                }
                PasDesPieces::Theme(t) => {
                    if let Ok(mut c) = self.chef.lock() {
                        c.theme = t;
                    }
                    0.0
                }
                PasDesPieces::Taille(l, h) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(l, h)));
                    0.0
                }
                PasDesPieces::Selecteur(noms) => {
                    let (fenetre, source) = (crate::natif::pieces::fenetre_d_eve(), self.source.clone());
                    self.selecteur = Some(thread::spawn(move || remplir_le_selecteur(fenetre, &source, noms)));
                    0.0
                }
                PasDesPieces::FinDuSelecteur => {
                    let depuis = *self.attente.get_or_insert(maintenant);
                    match self.selecteur.take_if(|f| f.is_finished() || maintenant - depuis > 20.0) {
                        Some(fil) if fil.is_finished() => match fil.join() {
                            Ok(Ok(())) => println!("la boîte de dialogue : remplie, puis validée"),
                            Ok(Err(e)) => self.probleme(format!("la boîte de dialogue : {e}")),
                            Err(_) => self.probleme("la boîte de dialogue : son fil a paniqué".into()),
                        },
                        Some(_) => self.probleme("la boîte de dialogue : son fil ne finit pas".into()),
                        None if self.selecteur.is_some() => {
                            self.parcours.push_front(PasDesPieces::FinDuSelecteur);
                            return;
                        }
                        None => {}
                    }
                    self.attente = None;
                    0.0
                }
                PasDesPieces::Capture(nom) => {
                    self.a_capturer.push(nom);
                    0.3
                }
                PasDesPieces::Fin => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    0.0
                }
            };
            self.prochain = maintenant + pause;
        }
    }
}

impl eframe::App for EssaiDesPieces {
    fn raw_input_hook(&mut self, ctx: &egui::Context, entree: &mut egui::RawInput) {
        // Seuls les gestes du parcours : la vraie souris qui passerait sur la fenêtre (Windows lui envoie ses
        // mouvements, même inactive) n'y fait rien.
        entree.events.retain(|e| {
            !matches!(
                e,
                egui::Event::PointerMoved(_)
                    | egui::Event::MouseMoved(_)
                    | egui::Event::PointerButton { .. }
                    | egui::Event::PointerGone
                    | egui::Event::MouseWheel { .. }
                    | egui::Event::Key { .. }
                    | egui::Event::Text(_)
            )
        });
        entree.focused = true;
        // Les fichiers glissés restent sur la fenêtre d'une image à l'autre, comme ceux que Windows y glisse.
        entree.hovered_files =
            self.survoles.iter().map(|p| egui::HoveredFile { path: Some(p.clone()), ..Default::default() }).collect();
        self.avancer(ctx, entree);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.fenetre, ui, frame);
        let ctx = ui.ctx().clone();
        for nom in self.a_capturer.drain(..) {
            if let Some(dossier) = &self.captures {
                let chemin = dossier.join(format!("pieces-{nom}.png"));
                ctx.request_screenshot(move |image| match enregistrer_png(&image, &chemin) {
                    Ok(()) => println!("capture : {}", chemin.display()),
                    Err(e) => eprintln!("capture {} : {e}", chemin.display()),
                });
            }
        }
        // Le parcours se lit à chaque image : soixante par seconde.
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        eframe::App::on_exit(&mut self.fenetre, gl);
    }
}

/// La boîte de dialogue du trombone, remplie par l'essai comme la personne le ferait : le dossier des fichiers
/// inventés, puis leurs noms, et « Ouvrir ». Sous Windows seulement.
fn remplir_le_selecteur(fenetre: isize, source: &Path, noms: &[&str]) -> Result<(), String> {
    #[cfg(windows)]
    {
        selecteur_des_pieces::remplir(fenetre, source, noms)
    }
    #[cfg(not(windows))]
    {
        let _ = (fenetre, source, noms);
        Err("la boîte de dialogue n'existe que sous Windows".into())
    }
}

/// La boîte de dialogue « Ouvrir » que le trombone de l'essai vient d'ouvrir : l'essai ne touche qu'à elle — une boîte
/// de dialogue de ce programme, attachée à sa fenêtre, et à aucune autre.
#[cfg(windows)]
mod selecteur_des_pieces {
    use std::path::Path;
    use std::thread;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, FindWindowExW, GW_OWNER, GetClassNameW, GetDlgCtrlID, GetWindow, GetWindowThreadProcessId,
        IsWindow, IsWindowVisible, SendMessageW, WM_CLOSE, WM_COMMAND, WM_SETTEXT,
    };
    use windows::core::{BOOL, PCWSTR, w};

    /// Le bouton « Ouvrir » et la case du nom des fichiers : leurs numéros de toujours (IDOK, cmb13).
    const OUVRIR: i32 = 1;
    const NOM_DES_FICHIERS: i32 = 0x047C;

    /// Ce qu'on cherche parmi les fenêtres de la boîte de dialogue : sa classe et son numéro ; et ce qu'on a trouvé.
    struct Recherche {
        classe: &'static str,
        numero: i32,
        trouvee: Option<HWND>,
    }

    /// Une fenêtre de la boîte de dialogue, vue par `EnumChildWindows` : la bonne arrête la recherche.
    unsafe extern "system" fn voir(h: HWND, recherche: LPARAM) -> BOOL {
        // SAFETY : `recherche` est la `Recherche` de `descendante`, vivante le temps de l'énumération ; des lectures.
        unsafe {
            let r = &mut *(recherche.0 as *mut Recherche);
            let mut classe = [0u16; 64];
            let n = usize::try_from(GetClassNameW(h, &mut classe)).unwrap_or(0);
            if GetDlgCtrlID(h) == r.numero && String::from_utf16_lossy(&classe[..n]) == r.classe {
                r.trouvee = Some(h);
                return BOOL(0);
            }
        }
        BOOL(1)
    }

    /// La fenêtre de cette classe et de ce numéro, n'importe où sous `parent` : la boîte de dialogue de Windows range
    /// sa case du nom loin sous elle, là où `GetDlgItem` ne cherche pas.
    fn descendante(parent: HWND, classe: &'static str, numero: i32) -> Option<HWND> {
        let mut r = Recherche { classe, numero, trouvee: None };
        // SAFETY : les fenêtres de la boîte de dialogue de ce programme ; `r` vit jusqu'au retour.
        let _ = unsafe { EnumChildWindows(Some(parent), Some(voir), LPARAM(&raw mut r as isize)) };
        r.trouvee
    }

    /// La boîte de dialogue visible de ce programme, attachée à `fenetre`.
    fn trouver(fenetre: HWND) -> Option<HWND> {
        // SAFETY : des lectures sur les fenêtres de premier niveau du poste ; rien ne s'y écrit.
        unsafe {
            let moi = GetCurrentProcessId();
            let mut apres = None;
            while let Ok(h) = FindWindowExW(None, apres, w!("#32770"), PCWSTR::null()) {
                let mut processus = 0;
                GetWindowThreadProcessId(h, Some(&raw mut processus));
                let attachee = GetWindow(h, GW_OWNER).is_ok_and(|p| p == fenetre);
                if processus == moi && attachee && IsWindowVisible(h).as_bool() {
                    return Some(h);
                }
                apres = Some(h);
            }
        }
        None
    }

    /// `texte` dans la case du nom des fichiers, puis « Ouvrir » (la commande du bouton, que la boîte de dialogue
    /// prend même quand elle n'est pas devant).
    fn ouvrir(dialogue: HWND, texte: &str) -> Result<(), String> {
        let large: Vec<u16> = texte.encode_utf16().chain([0]).collect();
        let case = descendante(dialogue, "ComboBoxEx32", NOM_DES_FICHIERS).ok_or("sa case du nom est introuvable")?;
        let bouton = descendante(dialogue, "Button", OUVRIR).ok_or("son bouton « Ouvrir » est introuvable")?;
        // SAFETY : la case et le bouton de la boîte de dialogue de ce programme ; le texte vit jusqu'au retour de
        // SendMessageW, qui attend que la boîte de dialogue l'ait pris.
        unsafe {
            let liste = FindWindowExW(Some(case), None, w!("ComboBox"), PCWSTR::null())
                .map_err(|e| format!("la liste de sa case : {e}"))?;
            let champ = FindWindowExW(Some(liste), None, w!("Edit"), PCWSTR::null())
                .map_err(|e| format!("le champ de sa case : {e}"))?;
            SendMessageW(champ, WM_SETTEXT, None, Some(LPARAM(large.as_ptr() as isize)));
            SendMessageW(dialogue, WM_COMMAND, Some(WPARAM(OUVRIR as usize)), Some(LPARAM(bouton.0 as isize)));
        }
        Ok(())
    }

    /// Elle s'ouvre (dix secondes au plus), l'essai y va au dossier des fichiers, les y choisit, et l'attend fermée ;
    /// encore ouverte cinq secondes après, elle se ferme sans rien rendre.
    pub(super) fn remplir(fenetre: isize, dossier: &Path, noms: &[&str]) -> Result<(), String> {
        let fenetre = HWND(fenetre as *mut core::ffi::c_void);
        let debut = Instant::now();
        let dialogue = loop {
            if let Some(d) = trouver(fenetre) {
                break d;
            }
            if debut.elapsed() > Duration::from_secs(10) {
                return Err("elle ne s'est pas ouverte".into());
            }
            thread::sleep(Duration::from_millis(100));
        };
        println!("la boîte de dialogue s'ouvre, en {:.1} s", debut.elapsed().as_secs_f64());
        thread::sleep(Duration::from_millis(700));
        let choix = noms.iter().map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(" ");
        let fait = ouvrir(dialogue, &dossier.display().to_string()).and_then(|()| {
            println!("la boîte de dialogue va au dossier des fichiers inventés");
            thread::sleep(Duration::from_millis(1000));
            println!("la boîte de dialogue : {choix}");
            ouvrir(dialogue, &choix)
        });
        let debut = Instant::now();
        // SAFETY : une lecture.
        while unsafe { IsWindow(Some(dialogue)) }.as_bool() {
            if debut.elapsed() > Duration::from_secs(5) {
                // SAFETY : la boîte de dialogue de ce programme se ferme comme par sa croix.
                unsafe { SendMessageW(dialogue, WM_CLOSE, None, None) };
                return fait.and(Err("elle ne s'est pas fermée".into()));
            }
            thread::sleep(Duration::from_millis(100));
        }
        fait
    }
}

/// Une pièce envoyée à la vitrine comme la page l'envoie (`POST /piece?nom=`, l'en-tête `X-Eve`) : sa réponse.
fn envoyer_comme_la_page(port: u16, jeton: &str, nom: &str, corps: &[u8]) -> String {
    use std::io::{Read, Write};
    let envoi = || -> std::io::Result<String> {
        let mut f = std::net::TcpStream::connect(("127.0.0.1", port))?;
        let longueur = corps.len();
        let tete = format!(
            "POST /piece?nom={nom} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nX-Eve: {jeton}\r\n\
             Content-Length: {longueur}\r\n\r\n"
        );
        f.write_all(tete.as_bytes())?;
        f.write_all(corps)?;
        let mut reponse = Vec::new();
        f.read_to_end(&mut reponse)?;
        Ok(String::from_utf8_lossy(&reponse).into_owned())
    };
    envoi().unwrap_or_else(|e| format!("pas de réponse : {e}"))
}

/// Le bilan de l'essai des pièces : chaque vérification, « ok » ou « ÉCHEC » ; 0 si tout va.
fn bilan_des_pieces(
    chef: &ChefDesPieces,
    source: &Path,
    problemes: &[String],
    selecteur: bool,
    (page, page_vide): (&str, &str),
) -> i32 {
    // Un nom reçu est celui du fichier ou, déjà pris dans `pieces\` (un essai d'avant), « nom (2).ext »…
    let meme = |recu: &str, original: &str| {
        let (tige, ext) = original.rsplit_once('.').map_or((original, String::new()), |(t, e)| (t, format!(".{e}")));
        recu == original || (recu.starts_with(&format!("{tige} (")) && recu.ends_with(&format!("){ext}")))
    };
    let recues: Vec<(&str, &Path, u64)> = chef
        .recus
        .iter()
        .filter_map(|r| match r {
            RecuDesPieces::Piece { nom, chemin, taille } => Some((nom.as_str(), chemin.as_path(), *taille)),
            _ => None,
        })
        .collect();
    let aujourdhui = crate::journal::date();
    let taille_de = |nom: &str| FICHIERS_DES_PIECES.iter().find(|(n, _)| *n == nom).map_or(0, |(_, t)| *t);
    // Les pièces reçues à partir de la `de`-ième : ces fichiers, à leur taille, copiés dans `pieces\AAAA-MM-JJ`.
    let jointes = |de: usize, noms: &[&str]| {
        recues.len() >= de + noms.len()
            && noms.iter().all(|n| {
                recues[de..de + noms.len()].iter().any(|(recu, chemin, taille)| {
                    let jour = chemin.parent().and_then(Path::file_name).is_some_and(|j| j == aujourdhui.as_str());
                    meme(recu, n) && *taille == taille_de(n) && jour
                })
            })
    };
    let refus: Vec<&str> = chef
        .recus
        .iter()
        .filter_map(|r| if let RecuDesPieces::Refus(e) = r { Some(e.as_str()) } else { None })
        .collect();
    let demande = chef.recus.iter().find_map(|r| match r {
        RecuDesPieces::Demande(_, noms) => Some(noms.clone()),
        _ => None,
    });
    let autres: Vec<&str> = chef
        .recus
        .iter()
        .filter_map(|r| if let RecuDesPieces::Autre(a) = r { Some(a.as_str()) } else { None })
        .filter(|a| *a != "quitter")
        .collect();
    let mut verifications = vec![
        (
            "trois fichiers déposés : trois pièces au chef, copiées dans pieces\\AAAA-MM-JJ du dossier local",
            jointes(0, &[FICHIERS_DES_PIECES[0].0, FICHIERS_DES_PIECES[1].0, FICHIERS_DES_PIECES[2].0]),
        ),
        (
            "plus de 40 Mo, vide, un dossier : refusés, et le chef l'écrit au fil",
            refus
                == [
                    "« trop-gros.bin » dépasse 40 Mo : non joint.",
                    "« vide.txt » est vide : non joint.",
                    "« un dossier » n'a pas pu être joint.",
                ],
        ),
        (
            "le × d'une pastille : `piece_retirer` et son numéro ; le chef efface son fichier",
            chef.recus.contains(&RecuDesPieces::Retrait { id: "p2".into(), efface: true }),
        ),
        (
            "une demande écrite emporte les pièces qui restent",
            demande.is_some_and(|noms| {
                noms.len() == 2 && meme(&noms[0], FICHIERS_DES_PIECES[0].0) && meme(&noms[1], FICHIERS_DES_PIECES[2].0)
            }),
        ),
        ("le centre de commande : la pièce déposée se joint", jointes(3, &[FICHIERS_DES_PIECES[3].0])),
        (
            "le thème « Constructo AI » : deux fichiers déposés, deux pièces",
            jointes(4, &[FICHIERS_DES_PIECES[3].0, FICHIERS_DES_PIECES[1].0]),
        ),
    ];
    if selecteur {
        let choisies = jointes(6, &[FICHIERS_DES_PIECES[0].0, FICHIERS_DES_PIECES[2].0]);
        verifications.push(("le trombone : sa boîte de dialogue, deux fichiers choisis, deux pièces", choisies));
    }
    // L'image collée : la dernière pièce de la fenêtre, un PNG nommé d'après l'heure.
    let collee = recues.iter().rev().find(|(nom, _, _)| nom.starts_with("Capture ")).is_some_and(|(nom, chemin, _)| {
        nom.ends_with(".png") && std::fs::read(chemin).is_ok_and(|o| o.starts_with(b"\x89PNG"))
    });
    verifications.push(("une image collée (Ctrl+V) : une pièce, en PNG", collee));
    let par_la_page = recues.iter().any(|(nom, _, taille)| *nom == "depuis-la-page.txt" && *taille == 25);
    let vide_refusee = page_vide.ends_with("« vide-par-la-page.txt » est vide : non joint.");
    verifications.extend([
        ("la page (POST /piece) : la même fonction joint sa pièce", page.starts_with("HTTP/1.1 200") && par_la_page),
        (
            "la page : une pièce vide refusée, son texte dans la réponse",
            page_vide.starts_with("HTTP/1.1 413") && vide_refusee,
        ),
        ("aucun autre geste au chef", autres.is_empty()),
        ("le parcours va jusqu'au bout", problemes.is_empty()),
    ]);
    println!("\nLes pièces jointes de la fenêtre native ({}) :", source.display());
    for (quoi, ok) in &verifications {
        println!("  {} {quoi}", if *ok { "ok   " } else { "ÉCHEC" });
    }
    for p in problemes {
        println!("        {p}");
    }
    if !autres.is_empty() {
        println!("        autres gestes : {}", autres.join(", "));
    }
    i32::from(!verifications.iter().all(|(_, ok)| *ok))
}

// ---- Les profils de la fenêtre native (profils.rs, natif\profils.rs) ----------------------------------------------

/// Les profils inventés de l'essai : celui d'office d'abord ; des dossiers d'exemple, que rien ne lit.
const PROFILS_D_ESSAI: [(&str, &str, bool); 3] = [
    ("Essai", r"C:\Exemple\Essai", true),
    ("Estimateur", r"C:\Exemple\Estimation", false),
    ("Designer", r"C:\Exemple\Design", false),
];

/// `eve --essai-profils [--captures DOSSIER]` : la vraie fenêtre native, sur un état inventé que l'essai lui publie
/// comme le chef le ferait — trois profils, dont il prend le geste `profil` comme lui : le fil le dit, la barre et les
/// réglages cochent le nouveau —, et ses profils mis à l'épreuve sans la souris de la personne : le nom sous « EVE »
/// ouvre leur menu, un profil s'y choisit ; le menu ouvert reste à egui pour Windows (pas à la barre de titre, qui
/// déplacerait la fenêtre) ; la rubrique « Profil » des réglages choisit aussi ; puis le même menu dans le thème
/// « Constructo AI », et ses réglages défilés jusqu'au portrait d'Eve. Le bilan dit ce qui va. `--captures` : la fenêtre, par elle-même, en PNG, à chaque étape. La
/// musique du centre se tait (A, S) avant de jouer.
pub fn profils(args: &[String]) -> i32 {
    let captures = match args {
        [] => None,
        [option, dossier] if option == "--captures" => Some(std::path::PathBuf::from(dossier)),
        _ => {
            eprintln!("eve --essai-profils [--captures DOSSIER]");
            return 2;
        }
    };
    if let Some(c) = &captures
        && let Err(e) = std::fs::create_dir_all(c)
    {
        eprintln!("{} : {e}", c.display());
        return 1;
    }
    let (rapport, recus) = mpsc::channel::<Evt>();
    let ecran = match crate::ecran::demarrer(0, crate::ecran::jeton_aleatoire(), rapport.clone()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("la vitrine de l'essai ne s'ouvre pas : {e}");
            return 1;
        }
    };
    let chef = Arc::new(Mutex::new(ChefDesProfils::nouveau()));
    ecran.publier(&chef.lock().map(|c| c.json()).unwrap_or_default());
    let abonnement = ecran.abonner();
    let fil_du_chef = {
        let (ecran, chef) = (ecran.clone(), chef.clone());
        thread::spawn(move || mener_le_chef_des_profils(&ecran, &recus, &chef))
    };
    let titre = format!("{} · Constructo AI", crate::fenetre::titre_pour("Essai des profils"));
    let vue = egui::ViewportBuilder::default()
        .with_title(&titre)
        .with_inner_size([1240.0, 800.0])
        .with_min_inner_size([400.0, 540.0])
        .with_active(false);
    let options = eframe::NativeOptions { viewport: vue, ..Default::default() };
    let problemes = Arc::new(Mutex::new(Vec::<String>::new()));
    let zones = Arc::new(Mutex::new(Vec::<(String, bool)>::new()));
    let (chef_de_l_essai, problemes_de_l_essai, zones_de_l_essai) = (chef.clone(), problemes.clone(), zones.clone());
    let creer: eframe::AppCreator<'_> = Box::new(move |cc| {
        Ok(Box::new(EssaiDesProfils {
            fenetre: crate::natif::Fenetre::nouvelle(cc, abonnement, rapport, None),
            chef: chef_de_l_essai,
            parcours: parcours_des_profils(),
            debut: Instant::now(),
            prochain: 0.0,
            attente: None,
            pointeur: egui::Pos2::ZERO,
            captures,
            a_capturer: Vec::new(),
            problemes: problemes_de_l_essai,
            zones: zones_de_l_essai,
        }))
    });
    if let Err(e) = eframe::run_native(&titre, options, creer) {
        eprintln!("la fenêtre ne s'ouvre pas : {e}");
        return 1;
    }
    thread::sleep(Duration::from_millis(300));
    if let Ok(mut c) = chef.lock() {
        c.fini = true;
    }
    let _ = fil_du_chef.join();
    let problemes = problemes.lock().map(|p| p.clone()).unwrap_or_default();
    let zones = zones.lock().map(|z| z.clone()).unwrap_or_default();
    let Ok(c) = chef.lock() else { return 1 };
    let mut verifications = vec![
        (
            "le menu de la barre, la rubrique des réglages, le menu du thème clair : le geste `profil` et son nom"
                .to_string(),
            c.choix == ["Estimateur", "Essai", "Designer"],
        ),
        ("aucun autre geste au chef".to_string(), c.autres.iter().all(|a| a == "quitter")),
    ];
    verifications.extend(zones);
    verifications.push(("le parcours va jusqu'au bout".to_string(), problemes.is_empty()));
    println!("\nLes profils de la fenêtre native :");
    for (quoi, ok) in &verifications {
        println!("  {} {quoi}", if *ok { "ok   " } else { "ÉCHEC" });
    }
    for p in &problemes {
        println!("        {p}");
    }
    if !c.choix.is_empty() {
        println!("        profils choisis : {}", c.choix.join(", "));
    }
    if c.autres.iter().any(|a| a != "quitter") {
        println!("        autres gestes : {}", c.autres.join(", "));
    }
    i32::from(!verifications.iter().all(|(_, ok)| *ok))
}

/// Le chef de l'essai des profils : il tient le profil actif et le fil comme le vrai, prend le geste `profil` comme
/// lui (`Chef::choisir_le_profil`, puis son passage) et publie son état.
struct ChefDesProfils {
    actif: String,
    theme: &'static str,
    fil: Vec<Value>,
    /// Les profils choisis par la fenêtre, dans l'ordre ; ses autres gestes.
    choix: Vec<String>,
    autres: Vec<String>,
    fini: bool,
}

impl ChefDesProfils {
    fn nouveau() -> ChefDesProfils {
        let bonjour = "Le nom sous « EVE » ouvre le menu des profils.";
        ChefDesProfils {
            actif: PROFILS_D_ESSAI[0].0.to_string(),
            theme: "centre",
            fil: vec![
                serde_json::json!({ "de": "eve", "texte": bonjour, "heure": "09:30", "pieces": [], "ancien": false }),
            ],
            choix: Vec::new(),
            autres: Vec::new(),
            fini: false,
        }
    }

    fn recevoir(&mut self, evt: Evt) {
        let Evt::Ui { action, valeur } = evt else { return };
        println!("geste : {action} {valeur}");
        match (action.as_str(), valeur.as_str()) {
            ("profil", Some(nom)) if PROFILS_D_ESSAI.iter().any(|(n, _, _)| *n == nom) => {
                self.actif = nom.to_string();
                let ligne = format!("— Profil {nom} —");
                self.fil.push(serde_json::json!({ "de": "systeme", "texte": ligne, "heure": "", "pieces": [] }));
                self.choix.push(nom.to_string());
            }
            ("theme", Some(t)) => {
                self.theme = crate::reglages::THEMES.iter().find(|x| x.0 == t).map_or("centre", |x| x.0)
            }
            _ => self.autres.push(action),
        }
    }

    fn json(&self) -> String {
        let profils: Vec<Value> = PROFILS_D_ESSAI
            .iter()
            .map(|(nom, dossier, office)| serde_json::json!({ "nom": nom, "dossier": dossier, "office": office }))
            .collect();
        serde_json::json!({
            "etat": "pret",
            "libelle": "Prête",
            "aide": "Dis « Eve… » — ou maintiens [Ctrl droit] pour parler",
            "niveau": 0.0,
            "qui": "Sam",
            "theme": self.theme,
            "nom": PROFILS_D_ESSAI[0].0,
            "profil": self.actif,
            "profils": profils,
            "propre": true,
            "entreprise": "",
            "base": "",
            "micro": "Le micro de l'essai",
            "oreille": "prete",
            "accord": Value::Null,
            "activite": [],
            "outils": [],
            "pieces": [],
            "fil_rev": self.fil.len(),
            "fil": self.fil,
            "en_cours": Value::Null,
            "voix": { "liste": [{ "id": "essai", "nom": "La voix de l'essai" }], "choisie": "essai" },
            "reglages": {
                "eveil": true,
                "interruption": true,
                "sourdine": true,
                "alertes": { "suspects": false, "factures": false, "pieces": false, "rendez_vous": false },
                "devant": false,
                "demarrage": false,
                "historique": false,
                "raccourci": "Ctrl+Alt+E",
            },
            "eveil": true,
            "touche": "Ctrl droit",
            "camera": false,
            "camera_ordre": { "n": 0, "geste": "" },
        })
        .to_string()
    }
}

/// Le fil du chef de l'essai : ce que la fenêtre envoie, et l'état publié quand il change.
fn mener_le_chef_des_profils(ecran: &crate::ecran::Ecran, recus: &mpsc::Receiver<Evt>, chef: &Mutex<ChefDesProfils>) {
    let mut publie = String::new();
    loop {
        match recus.recv_timeout(Duration::from_millis(40)) {
            Ok(evt) => {
                if let Ok(mut c) = chef.lock() {
                    c.recevoir(evt);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let json = match chef.lock() {
            Ok(c) if !c.fini => c.json(),
            _ => return,
        };
        if json != publie {
            ecran.publier(&json);
            publie = json;
        }
    }
}

/// Un pas du parcours de l'essai des profils.
enum PasDesProfils {
    Pause(f64),
    /// Le pointeur sur la cible ; son bouton qui s'enfonce, qui se relâche.
    Survol(CibleDesProfils),
    Appuyer,
    Relacher,
    Touches(&'static [egui::Key]),
    /// Le chef de l'essai a pris ce profil (le parcours l'attend six secondes au plus).
    Actif(&'static str),
    /// Le menu ouvert, ce que Windows en saurait : le nom qui l'ouvre et le menu lui-même, à egui.
    MenuAEgui,
    Reglages(bool),
    Theme(&'static str),
    /// La molette sous le pointeur (en points ; négatif : vers le bas).
    Molette(f32),
    Capture(&'static str),
    Fin,
}

/// Ce que vise le pointeur de l'essai : le nom sous « EVE », un profil du menu, un profil des réglages.
#[derive(Debug)]
enum CibleDesProfils {
    Menu,
    DuMenu(&'static str),
    DesReglages(&'static str),
}

/// Le parcours : le menu de la barre (ouvert, survolé, un profil choisi), la rubrique des réglages (celui d'office
/// choisi), puis le thème « Constructo AI » et son menu (un troisième profil), et ses réglages défilés jusqu'au thème
/// (le portrait d'Eve, qu'il est seul à montrer).
fn parcours_des_profils() -> VecDeque<PasDesProfils> {
    use CibleDesProfils::{DesReglages, DuMenu, Menu};
    use PasDesProfils::*;
    VecDeque::from([
        Touches(&[egui::Key::A, egui::Key::S]),
        Pause(1.2),
        Capture("01-barre"),
        Survol(Menu),
        Pause(0.5),
        Capture("02-survol"),
        Appuyer,
        Relacher,
        Pause(0.7),
        MenuAEgui,
        Capture("03-menu"),
        Survol(DuMenu("Estimateur")),
        Pause(0.5),
        Capture("04-menu-survol"),
        Appuyer,
        Relacher,
        Actif("Estimateur"),
        Pause(0.8),
        Capture("05-estimateur"),
        Reglages(true),
        Pause(1.0),
        Capture("06-reglages"),
        Survol(DesReglages("Essai")),
        Pause(0.3),
        Appuyer,
        Relacher,
        Actif("Essai"),
        Pause(0.8),
        Capture("07-reglages-office"),
        Reglages(false),
        Theme("constructo"),
        Pause(1.6),
        Survol(Menu),
        Pause(0.3),
        Appuyer,
        Relacher,
        Pause(0.7),
        Capture("08-clair-menu"),
        Survol(DuMenu("Designer")),
        Pause(0.3),
        Appuyer,
        Relacher,
        Actif("Designer"),
        Pause(0.8),
        Capture("09-clair-designer"),
        Reglages(true),
        Pause(1.0),
        Survol(DesReglages("Designer")),
        Molette(-700.0),
        Pause(0.8),
        Capture("10-clair-reglages"),
        Reglages(false),
        Fin,
    ])
}

/// La fenêtre de l'essai des profils : la vraie fenêtre d'Eve, et le parcours qui lui donne ses gestes.
struct EssaiDesProfils {
    fenetre: crate::natif::Fenetre,
    chef: Arc<Mutex<ChefDesProfils>>,
    /// Le parcours, son départ, l'heure de son prochain pas (en secondes), depuis quand il attend le chef ; le pointeur
    /// qu'il promène ; les captures à faire.
    parcours: VecDeque<PasDesProfils>,
    debut: Instant,
    prochain: f64,
    attente: Option<f64>,
    pointeur: egui::Pos2,
    captures: Option<std::path::PathBuf>,
    a_capturer: Vec<&'static str>,
    /// Ce qui n'a pas marché ; ce que dit la barre de titre du menu ouvert, pour le bilan.
    problemes: Arc<Mutex<Vec<String>>>,
    zones: Arc<Mutex<Vec<(String, bool)>>>,
}

impl EssaiDesProfils {
    fn position(ctx: &egui::Context, cible: &CibleDesProfils) -> Option<egui::Pos2> {
        let id = match cible {
            CibleDesProfils::Menu => crate::natif::profils::id_du_menu_des_profils(),
            CibleDesProfils::DuMenu(nom) => crate::natif::profils::id_du_profil(nom),
            CibleDesProfils::DesReglages(nom) => crate::natif::profils::id_du_profil_des_reglages(nom),
        };
        ctx.read_response(id).map(|r| r.rect.center())
    }

    fn probleme(&self, quoi: String) {
        eprintln!("essai : {quoi}");
        if let Ok(mut p) = self.problemes.lock() {
            p.push(quoi);
        }
    }

    /// Le menu ouvert : est-il ouvert, et ce qui s'y clique — le nom qui l'ouvre, le haut du menu là où il passe sur la
    /// barre — est-il à egui pour Windows (`cadre::zone`), et non à la barre de titre ?
    fn menu_a_egui(&self, ctx: &egui::Context) {
        let mut notes = Vec::new();
        let bouton = ctx.read_response(crate::natif::profils::id_du_menu_des_profils());
        let id_du_menu = crate::natif::profils::id_du_menu_des_profils().with("popup");
        let ouvert = egui::Popup::is_id_open(ctx, id_du_menu);
        notes.push(("le nom sous « EVE » ouvre le menu des profils".to_string(), ouvert));
        let menu = ctx.memory(|m| m.area_rect(id_du_menu));
        match (crate::cadre::barre(), bouton) {
            (Some(barre), Some(bouton)) => {
                let largeur = ctx.content_rect().width();
                let a_egui =
                    |p: egui::Pos2| crate::cadre::zone(&barre, p, largeur, 0.0, false) == crate::cadre::Zone::Client;
                notes.push((
                    "le nom qui ouvre le menu est noté parmi ce qui se clique dans la barre".to_string(),
                    a_egui(bouton.rect.center()),
                ));
                // Le haut du menu, s'il passe sur la barre (sinon, rien à vérifier : sous elle, tout est à egui).
                let haut = menu.map(|m| egui::pos2(m.center().x, m.top() + 1.0));
                notes.push((
                    "le menu ouvert, là où il passe sur la barre, reste à egui".to_string(),
                    haut.is_some_and(|h| h.y >= barre.hauteur || a_egui(h)),
                ));
            }
            (barre, bouton) => self.probleme(format!(
                "la barre de titre ({}) ou le nom sous « EVE » ({}) introuvable",
                if barre.is_some() { "là" } else { "absente" },
                if bouton.is_some() { "là" } else { "absent" }
            )),
        }
        if let Ok(mut z) = self.zones.lock() {
            z.extend(notes);
        }
    }

    /// Les pas venus à leur heure, en événements d'egui ; un pas qui attend le chef retient la suite.
    fn avancer(&mut self, ctx: &egui::Context, entree: &mut egui::RawInput) {
        let maintenant = self.debut.elapsed().as_secs_f64();
        let rien = egui::Modifiers::NONE;
        let touche =
            |key, pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: rien };
        while maintenant >= self.prochain {
            let Some(pas) = self.parcours.pop_front() else { return };
            let pause = match pas {
                PasDesProfils::Pause(s) => s,
                PasDesProfils::Survol(cible) => match Self::position(ctx, &cible) {
                    Some(p) => {
                        self.pointeur = p;
                        entree.events.push(egui::Event::PointerMoved(p));
                        0.15
                    }
                    None => {
                        self.probleme(format!("{cible:?} introuvable"));
                        0.0
                    }
                },
                PasDesProfils::Appuyer | PasDesProfils::Relacher => {
                    entree.events.push(egui::Event::PointerButton {
                        pos: self.pointeur,
                        button: egui::PointerButton::Primary,
                        pressed: matches!(pas, PasDesProfils::Appuyer),
                        modifiers: rien,
                    });
                    0.12
                }
                PasDesProfils::Touches(touches) => {
                    entree.events.extend(touches.iter().flat_map(|&k| [touche(k, true), touche(k, false)]));
                    0.1
                }
                PasDesProfils::Actif(nom) => {
                    let actif = self.chef.lock().map_or(String::new(), |c| c.actif.clone());
                    if actif != nom {
                        let depuis = *self.attente.get_or_insert(maintenant);
                        if maintenant - depuis < 6.0 {
                            self.parcours.push_front(PasDesProfils::Actif(nom));
                            return;
                        }
                        self.probleme(format!("le profil {nom} attendu au chef, il tient {actif}"));
                    }
                    self.attente = None;
                    0.0
                }
                PasDesProfils::MenuAEgui => {
                    self.menu_a_egui(ctx);
                    0.0
                }
                PasDesProfils::Reglages(oui) => {
                    self.fenetre.reglages_ouverts = oui;
                    0.0
                }
                PasDesProfils::Theme(t) => {
                    if let Ok(mut c) = self.chef.lock() {
                        c.theme = t;
                    }
                    0.0
                }
                PasDesProfils::Molette(dy) => {
                    entree.events.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, dy),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Wheel,
                        modifiers: rien,
                    });
                    0.1
                }
                PasDesProfils::Capture(nom) => {
                    self.a_capturer.push(nom);
                    0.3
                }
                PasDesProfils::Fin => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    0.0
                }
            };
            self.prochain = maintenant + pause;
        }
    }
}

impl eframe::App for EssaiDesProfils {
    fn raw_input_hook(&mut self, ctx: &egui::Context, entree: &mut egui::RawInput) {
        // Seuls les gestes du parcours : la vraie souris qui passerait sur la fenêtre n'y fait rien.
        entree.events.retain(|e| {
            !matches!(
                e,
                egui::Event::PointerMoved(_)
                    | egui::Event::MouseMoved(_)
                    | egui::Event::PointerButton { .. }
                    | egui::Event::PointerGone
                    | egui::Event::MouseWheel { .. }
                    | egui::Event::Key { .. }
                    | egui::Event::Text(_)
            )
        });
        entree.focused = true;
        self.avancer(ctx, entree);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        eframe::App::ui(&mut self.fenetre, ui, frame);
        let ctx = ui.ctx().clone();
        for nom in self.a_capturer.drain(..) {
            if let Some(dossier) = &self.captures {
                let chemin = dossier.join(format!("profils-{nom}.png"));
                ctx.request_screenshot(move |image| match enregistrer_png(&image, &chemin) {
                    Ok(()) => println!("capture : {}", chemin.display()),
                    Err(e) => eprintln!("capture {} : {e}", chemin.display()),
                });
            }
        }
        // Le parcours se lit à chaque image : soixante par seconde.
        ctx.request_repaint_after(Duration::from_millis(16));
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        eframe::App::on_exit(&mut self.fenetre, gl);
    }
}

// ---- Le portrait d'Eve et ses poses -------------------------------------------------------------------------------

/// `eve --essai-portrait <dossier> [--captures DOSSIER] [--seules MOTS] [--rayon R] [--film DOSSIER [--voix DOSSIER]
/// [--conversation | --emotions]]` : le portrait d'Eve posé dans `dossier` (un dossier local : `portrait.png` et ses
/// poses), dans toutes ses poses côte à côte — la photo, chaque clignement, chaque regard, chaque bouche, chaque
/// émotion, des mélanges —, en grand comme la bannière les montre, sur son bleu : on y juge les calques et leurs bords.
/// `--seules` n'en garde que celles dont le nom porte l'un des mots dits (séparés par des virgules :
/// « penchée,mi-chemin »), plus grandes ; `--rayon` donne à chaque disque ce rayon (en points : 65, celui de la
/// bannière). `--captures` en fait une image (`portrait.png`, quand ses textures ont été refaites à sa taille) et une
/// de sa toute première (`portrait-premiere.png`, avec les textures entières), puis ferme la fenêtre. `--film` le
/// montre vivant, seul, dans un petit scénario ([`PHRASES_DU_FILM`] ; avec `--emotions`, [`PHRASES_DES_EMOTIONS`]), et
/// enregistre chacune de ses images à la cadence de l'écran (une vidéo s'en fait) ; `--voix` y prend des phrases dites
/// (`phrase-1.wav`… : leur texte est celui du scénario), à la place d'une voix inventée ; `--conversation` la fait
/// d'abord écouter, puis réfléchir.
pub fn portrait(args: &[String]) -> i32 {
    let valeur = |nom: &str| args.iter().position(|a| a == nom).and_then(|i| args.get(i + 1)).cloned();
    let captures = valeur("--captures").map(std::path::PathBuf::from);
    let film = valeur("--film").map(std::path::PathBuf::from);
    let voix = valeur("--voix").map(std::path::PathBuf::from);
    let emotions = args.iter().any(|a| a == "--emotions");
    let conversation = args.iter().any(|a| a == "--conversation");
    let seules: Option<Vec<String>> = valeur("--seules").map(|m| m.split(',').map(|m| m.trim().to_string()).collect());
    let rayon = valeur("--rayon").and_then(|r| r.parse::<f32>().ok()).filter(|r| *r > 4.0);
    let a_valeur = ["--captures", "--film", "--voix", "--seules", "--rayon"];
    let dossier = args
        .iter()
        .enumerate()
        .find(|(k, a)| !a.starts_with("--") && (*k == 0 || !a_valeur.contains(&args[k - 1].as_str())))
        .map(|(_, a)| std::path::PathBuf::from(a));
    let Some(dossier) = dossier else {
        eprintln!(
            "eve --essai-portrait <dossier> [--captures DOSSIER] [--seules MOTS] [--rayon R] [--film DOSSIER [--voix \
             DOSSIER] [--conversation | --emotions]] : le dossier local"
        );
        return 2;
    };
    for c in [&captures, &film].into_iter().flatten() {
        if let Err(e) = std::fs::create_dir_all(c) {
            eprintln!("{} : {e}", c.display());
            return 1;
        }
    }
    if let Some(film) = film {
        let d = voix.as_deref();
        let scenario = if emotions { scenario_des_emotions(d) } else { scenario_du_film(d, conversation) };
        return filmer_le_portrait(dossier, film, scenario);
    }
    let titre = "Eve — essai du portrait";
    let vue = egui::ViewportBuilder::default().with_title(titre).with_inner_size([1480.0, 960.0]).with_active(false);
    let options = eframe::NativeOptions { viewport: vue, ..Default::default() };
    let creer: eframe::AppCreator<'_> = Box::new(move |_| {
        Ok(Box::new(EssaiDuPortrait {
            portrait: crate::natif::portrait::Portrait::nouveau(dossier),
            captures,
            seules,
            rayon,
            debut: None,
            premiere: false,
            capture_a: None,
        }))
    });
    match eframe::run_native(titre, options, creer) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("la fenêtre ne s'ouvre pas : {e}");
            1
        }
    }
}

/// La fenêtre de l'essai du portrait : le portrait, les poses gardées (`--seules`), le rayon de leurs disques
/// (`--rayon`), l'heure de sa première image prête, les captures demandées (la toute première, puis l'autre et son
/// heure).
struct EssaiDuPortrait {
    portrait: crate::natif::portrait::Portrait,
    captures: Option<std::path::PathBuf>,
    seules: Option<Vec<String>>,
    rayon: Option<f32>,
    debut: Option<f64>,
    premiere: bool,
    capture_a: Option<f64>,
}

impl eframe::App for EssaiDuPortrait {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let zone = ui.max_rect();
        let p = ui.painter();
        p.rect_filled(zone, 0.0, crate::natif::theme::erp::AZUR);
        ctx.request_repaint_after(Duration::from_millis(50));
        let Some(visage) = self.portrait.visage(&ctx) else {
            p.text(
                zone.center(),
                egui::Align2::CENTER_CENTER,
                "…",
                egui::FontId::proportional(20.0),
                egui::Color32::WHITE,
            );
            return;
        };
        // Toutes les poses dans la fenêtre (ou celles qui sont gardées) : le nombre de colonnes qui leur laisse les plus
        // grands disques — ou, à un rayon donné, autant qu'il en tient en largeur.
        let mut poses = visage.poses_d_essai();
        if let Some(mots) = &self.seules {
            poses.retain(|(nom, _)| mots.iter().any(|m| nom.contains(m.as_str())));
        }
        let (cote, colonnes) = match self.rayon {
            Some(r) => (r / 0.4, (((zone.width() - 30.0) / (r / 0.4 + 30.0)) as usize).max(1)),
            None => (1..=12)
                .map(|c: usize| {
                    let rangees = poses.len().div_ceil(c) as f32;
                    let cote = ((zone.width() - 30.0) / c as f32 - 30.0).min((zone.height() - 40.0) / rangees - 50.0);
                    (cote, c)
                })
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap_or((210.0, 6)),
        };
        for (i, (nom, pose)) in poses.iter().enumerate() {
            let (c, r) = ((i % colonnes) as f32, (i / colonnes) as f32);
            let centre =
                zone.min + egui::vec2(30.0 + cote / 2.0 + c * (cote + 30.0), 40.0 + cote / 2.0 + r * (cote + 50.0));
            visage.en_grand(p, centre, cote * 0.4, pose);
            // Son nom, réduit au besoin pour tenir sous son disque : les voisins ne se chevauchent pas.
            let mesure =
                |taille: f32| p.layout_no_wrap(nom.clone(), egui::FontId::proportional(taille), egui::Color32::WHITE);
            let mut texte = mesure(15.0);
            if texte.size().x > cote + 26.0 {
                texte = mesure((15.0 * (cote + 26.0) / texte.size().x).max(9.0));
            }
            p.galley(centre + egui::vec2(-texte.size().x / 2.0, cote / 2.0), texte, egui::Color32::WHITE);
        }
        // Une image de la toute première, avec les textures entières ; une autre quand elles ont été refaites à la taille
        // des disques (quatre secondes au plus : un grand disque n'en a pas besoin) ; puis la fenêtre se ferme.
        let temps = ui.input(|i| i.time);
        let debut = *self.debut.get_or_insert(temps);
        if let Some(dossier) = &self.captures {
            let capturer = |nom: &str| {
                let chemin = dossier.join(nom);
                ctx.request_screenshot(move |image| match enregistrer_png(&image, &chemin) {
                    Ok(()) => println!("capture : {}", chemin.display()),
                    Err(e) => eprintln!("capture {} : {e}", chemin.display()),
                });
            };
            if !self.premiere {
                self.premiere = true;
                capturer("portrait-premiere.png");
            } else if self.capture_a.is_none() && temps - debut > 1.0 && (visage.est_net() || temps - debut > 4.0) {
                self.capture_a = Some(temps);
                capturer("portrait.png");
            } else if self.capture_a.is_some_and(|t| temps - t > 1.0) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

// ---- Le film du portrait ------------------------------------------------------------------------------------------

/// Les phrases du film de l'essai du portrait (`--film`), chacune avec l'émotion qui la dit : sans émotion, avec joie,
/// avec tristesse, amusée ; puis une question, sans émotion (un « non », un deux-points, une fin de phrase, une
/// question qui attend sa réponse).
const PHRASES_DU_FILM: [(Option<parole::Emotion>, &str); 5] = [
    (None, "Bonjour Sam. Je t'ai préparé le point de la journée : deux rendez-vous, et une facture à envoyer."),
    (Some(parole::Emotion::Joie), "Bonne nouvelle : le client a accepté la soumission. On commence lundi !"),
    (Some(parole::Emotion::Triste), "Je suis désolée : le chantier est retardé d'une semaine, à cause de la pluie."),
    (
        Some(parole::Emotion::Amusee),
        "Pourquoi les plongeurs plongent-ils en arrière ? Sinon, ils tombent dans le bateau !",
    ),
    (None, "Non, pas cette semaine : le fournisseur est en retard. Veux-tu que je l'appelle demain matin ?"),
];

/// Les phrases du film des émotions (`--film … --emotions`), comme Claude les écrit et que le chef les fait dire : une
/// réponse empathique, une mauvaise nouvelle, une qui change de ton en cours de route (le sérieux, puis la joie : le
/// deuxième passage suit le premier sans pause), une qui réfléchit tout haut. Chacune : son émotion, si elle suit la
/// phrase d'avant dans la même réponse, son texte.
const PHRASES_DES_EMOTIONS: [(parole::Emotion, bool, &str); 5] = [
    (parole::Emotion::Empathie, false, "Je comprends, ce n'est pas facile. On va trouver une solution ensemble."),
    (parole::Emotion::Triste, false, "Je suis désolée : le chantier est retardé d'une semaine."),
    (parole::Emotion::Serieuse, false, "Il faudra verser l'acompte avant vendredi."),
    (parole::Emotion::Joie, true, "Mais bonne nouvelle : le client a accepté la soumission !"),
    (parole::Emotion::Reflexion, false, "Il y a deux façons de voir ça : le prix, ou le délai."),
];

/// Une étape du film : de quand (en secondes), l'émotion que publie le chef, ce que fait Eve quand elle ne parle pas,
/// et la phrase qu'elle dit, s'il en dit une (son rang et sa piste).
struct EtapeDuFilm {
    debut: f64,
    emotion: Option<parole::Emotion>,
    activite: crate::natif::portrait::Activite,
    phrase: Option<(usize, voix::Piste)>,
}

/// La piste de la phrase `k` du film, de texte `texte` : dite (`phrase-1.wav`… dans `dites`), ou une voix inventée.
fn piste_du_film(dites: Option<&Path>, k: usize, texte: &str) -> voix::Piste {
    match dites.and_then(|d| std::fs::read(d.join(format!("phrase-{}.wav", k + 1))).ok()) {
        Some(wav) => voix::piste_du_son(&wav, texte),
        None => {
            let duree = Duration::from_secs_f64((parole::syllabes(texte).len() as f64 / 4.5).max(1.0));
            voix::piste_de(voix_inventee(duree), texte)
        }
    }
}

/// Le temps que met une phrase à s'entendre après que le chef a dit « parole » — dès qu'il la donne à la voix : Piper
/// la prépare, le lecteur de Windows l'ouvre ; de 0,3 à 1,3 s au journal de l'Assistante. Eve inspire pendant ce temps.
const AVANT_LE_SON: f64 = 0.6;

/// Le scénario du film : Eve au repos, puis chaque phrase — son émotion vient d'abord, une seconde ; une question, Eve
/// écoute d'abord la personne, puis réfléchit, et après l'avoir posée, écoute sa réponse ; le chef dit « parole » un
/// peu avant le son ([`AVANT_LE_SON`]) —, puis le calme revenu ; et sa fin. Une phrase dite (`phrase-1.wav`… dans
/// `dites`), ou une voix inventée. `conversation` : avant la première phrase aussi, Eve écoute la personne, puis
/// réfléchit, comme quand on lui parle.
fn scenario_du_film(dites: Option<&Path>, conversation: bool) -> (Vec<EtapeDuFilm>, f64) {
    use crate::natif::portrait::Activite;
    let etape = |debut, emotion, activite| EtapeDuFilm { debut, emotion, activite, phrase: None };
    let mut etapes = vec![etape(0.0, None, Activite::Repos)];
    let mut t = 1.5;
    for (k, &(emotion, texte)) in PHRASES_DU_FILM.iter().enumerate() {
        let question = texte.trim_end().ends_with('?');
        if question || (conversation && k == 0) {
            // La personne parle (trois secondes : [`voix_de_la_personne`]), puis Eve réfléchit quatre secondes (son air
            // pensif a le temps de se poser, et ses yeux de chercher).
            etapes.push(etape(t, None, Activite::Ecoute));
            etapes.push(etape(t + 3.5, None, Activite::Reflechit));
            t += 7.5;
        }
        if emotion.is_some() {
            etapes.push(etape(t, emotion, Activite::Repos));
            t += 1.2 - AVANT_LE_SON;
        }
        etapes.push(etape(t, emotion, Activite::Parle));
        t += AVANT_LE_SON;
        let piste = piste_du_film(dites, k, texte);
        let duree = piste.duree().as_secs_f64();
        let apres = if question { Activite::Ecoute } else { Activite::Repos };
        etapes.push(EtapeDuFilm { debut: t, emotion, activite: apres, phrase: Some((k, piste)) });
        t += duree + if question { 3.0 } else { 0.8 };
    }
    etapes.push(etape(t, None, Activite::Repos));
    (etapes, t + 2.5)
}

/// La voix de la personne qu'Eve écoute dans le film, `t` secondes après qu'elle s'est mise à l'écouter : le niveau du
/// micro (de 0 à 1, comme le chef le publie), deux élans de parole à cinq syllabes par seconde séparés d'une courte
/// pause, puis le silence.
fn voix_de_la_personne(t: f64) -> f32 {
    let parle = [(0.3, 1.6), (2.05, 2.9)].iter().any(|&(a, b)| (a..b).contains(&t));
    if parle { 0.45 + 0.35 * (t * 5.0 * std::f64::consts::PI).sin().abs() as f32 } else { 0.03 }
}

/// Le scénario du film des émotions ([`PHRASES_DES_EMOTIONS`]), comme une vraie Eve les vit : pour chaque réponse, Eve se
/// tait un moment après la précédente (elle garde son émotion le temps qu'il faut), la personne parle (l'émotion d'avant
/// encore publiée), Claude réfléchit (l'air pensif ; la dernière fois longtemps : il va et vient), la première phrase est
/// donnée à la voix (le chef publie son émotion, un instant avant le son), elle se dit ; le passage qui suit dans la même
/// réponse part dans le silence de fin de la phrase d'avant, avec son émotion (le chef la publie quand la phrase d'avant
/// finit).
fn scenario_des_emotions(dites: Option<&Path>) -> (Vec<EtapeDuFilm>, f64) {
    use crate::natif::portrait::Activite::{Ecoute, Parle, Reflechit, Repos};
    let mut etapes = vec![EtapeDuFilm { debut: 0.0, emotion: None, activite: Repos, phrase: None }];
    let (mut t, mut avant) = (1.5, None);
    for (k, &(emotion, enchainee, texte)) in PHRASES_DES_EMOTIONS.iter().enumerate() {
        let piste = piste_du_film(dites, k, texte);
        if enchainee {
            // La voix passe à cette phrase dans les 120 derniers ms de celle d'avant : le chef apprend là qu'elle a fini
            // et publie l'émotion du passage ; le son vient un dixième de seconde plus tard (le lecteur de Windows).
            t -= 0.12;
            etapes.push(EtapeDuFilm { debut: t, emotion: Some(emotion), activite: Parle, phrase: None });
            t += 0.1;
        } else {
            if k > 0 {
                etapes.push(EtapeDuFilm { debut: t, emotion: avant, activite: Repos, phrase: None });
                t += 3.0;
            }
            etapes.push(EtapeDuFilm { debut: t, emotion: avant, activite: Ecoute, phrase: None });
            t += 1.6;
            etapes.push(EtapeDuFilm { debut: t, emotion: None, activite: Reflechit, phrase: None });
            t += if k + 1 == PHRASES_DES_EMOTIONS.len() { 9.0 } else { 2.0 };
            etapes.push(EtapeDuFilm { debut: t, emotion: Some(emotion), activite: Parle, phrase: None });
            t += 0.4;
        }
        let duree = piste.duree().as_secs_f64();
        etapes.push(EtapeDuFilm { debut: t, emotion: Some(emotion), activite: Repos, phrase: Some((k, piste)) });
        t += duree;
        avant = Some(emotion);
    }
    etapes.push(EtapeDuFilm { debut: t, emotion: avant, activite: Repos, phrase: None });
    (etapes, t + 4.5)
}

/// Une image du film à écrire : son instant (en ms depuis la première), l'image de la fenêtre, et la part à garder (en
/// pixels : gauche, haut, droite, bas).
type ImageDuFilm = (u64, Arc<egui::ColorImage>, [usize; 4]);

/// Filme le portrait posé dans `dossier`, dans `film`, selon son `scenario` (ses étapes, et sa fin en secondes) : la
/// fenêtre du film ([`FilmDuPortrait`]), qui se ferme d'elle-même à la fin, et trois fils qui en écrivent les images
/// (un seul ne suivrait pas la cadence de l'écran).
fn filmer_le_portrait(
    dossier: std::path::PathBuf,
    film: std::path::PathBuf,
    (etapes, fin): (Vec<EtapeDuFilm>, f64),
) -> i32 {
    println!("film du portrait : {:.1} s, dans {}", fin, film.display());
    let (envoi, recu) = mpsc::channel::<ImageDuFilm>();
    let recu = Arc::new(Mutex::new(recu));
    let ecrivains = (0..3)
        .map(|_| {
            let (recu, film) = (recu.clone(), film.clone());
            thread::spawn(move || {
                while let Some((ms, image, [x0, y0, x1, y1])) = recu.lock().ok().and_then(|r| r.recv().ok()) {
                    let [l, h] = image.size;
                    let (x1, y1) = (x1.min(l), y1.min(h));
                    if x0 >= x1 || y0 >= y1 {
                        continue;
                    }
                    let pixels = (y0..y1).flat_map(|y| image.pixels[y * l + x0..y * l + x1].iter().copied()).collect();
                    let chemin = film.join(format!("{ms:06}.png"));
                    let part = egui::ColorImage::new([x1 - x0, y1 - y0], pixels);
                    if let Err(e) = ecrire_png(&part, &chemin, png::Compression::Fast) {
                        eprintln!("{} : {e}", chemin.display());
                    }
                }
            })
        })
        .collect();
    let titre = "Eve — film du portrait";
    let vue = egui::ViewportBuilder::default().with_title(titre).with_inner_size([440.0, 500.0]).with_active(false);
    let options = eframe::NativeOptions { viewport: vue, ..Default::default() };
    let creer: eframe::AppCreator<'_> = Box::new(move |_| {
        Ok(Box::new(FilmDuPortrait {
            portrait: crate::natif::portrait::Portrait::nouveau(dossier),
            etapes,
            fin,
            etape: None,
            debut: None,
            envoi: Some(envoi),
            ecrivains,
            rendu: Vec::new(),
        }))
    });
    match eframe::run_native(titre, options, creer) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("la fenêtre ne s'ouvre pas : {e}");
            1
        }
    }
}

/// La fenêtre du film du portrait : le portrait, son scénario et sa fin, l'étape en cours, l'heure de sa première
/// image ; les images à écrire, et les fils qui les écrivent ; ce que le processeur a mis à préparer chaque image du
/// portrait (en millisecondes), dit à la fin.
struct FilmDuPortrait {
    portrait: crate::natif::portrait::Portrait,
    etapes: Vec<EtapeDuFilm>,
    fin: f64,
    etape: Option<usize>,
    debut: Option<f64>,
    envoi: Option<mpsc::Sender<ImageDuFilm>>,
    ecrivains: Vec<thread::JoinHandle<()>>,
    rendu: Vec<f64>,
}

impl eframe::App for FilmDuPortrait {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let zone = ui.max_rect();
        ui.painter().rect_filled(zone, 0.0, crate::natif::theme::erp::AZUR);
        ctx.request_repaint();
        let Some(visage) = self.portrait.visage(&ctx) else { return };
        let temps = ui.input(|i| i.time);
        let t = temps - *self.debut.get_or_insert(temps);
        // L'étape du moment : à chacune, sa phrase se fait entendre (ou plus rien), comme la voix d'Eve le fait.
        use crate::natif::portrait::Activite;
        let k = self.etapes.iter().rposition(|e| e.debut <= t).unwrap_or(0);
        if self.etape != Some(k) {
            self.etape = Some(k);
            let phrase = self.etapes[k].phrase.as_ref();
            voix::faire_entendre(phrase.map(|(_, p)| p.clone()));
            match (phrase, self.etapes[k].activite) {
                (Some((n, _)), _) => {
                    println!("phrase {} : à {:.0} ms", n + 1, t * 1000.0);
                    // Ses repères, à leur instant dans le film : les gestes s'y accrochent.
                    let (_, reperes) = voix::reperes_a_venir(Duration::ZERO).unwrap_or_default();
                    let liste: Vec<String> = reperes
                        .iter()
                        .map(|r| format!("{:?} {:.0}", r.marque, (t + f64::from(r.a)) * 1000.0))
                        .collect();
                    println!("  repères : {}", liste.join(", "));
                }
                (None, Activite::Ecoute) => println!("elle écoute : à {:.0} ms", t * 1000.0),
                (None, Activite::Reflechit) => println!("elle réfléchit : à {:.0} ms", t * 1000.0),
                (None, Activite::Parle) => println!("elle va parler : à {:.0} ms", t * 1000.0),
                _ => {}
            }
        }
        let e = &self.etapes[k];
        let dite = e.phrase.as_ref().map_or(0.0, |(_, p)| p.duree().as_secs_f64());
        let activite = if t < e.debut + dite { Activite::Parle } else { e.activite };
        // La personne qu'elle écoute parle (le niveau du micro, depuis qu'elle écoute) : elle hoche à ses pauses.
        let ecoute = activite == Activite::Ecoute;
        self.portrait.entendre(if ecoute { voix_de_la_personne(t - e.debut - dite) } else { 0.0 });
        let pose = self.portrait.pose_du_moment(temps, activite, true, e.emotion).unwrap_or_default();
        let (centre, rayon) = (zone.center() + egui::vec2(0.0, 40.0), 150.0);
        let depart = Instant::now();
        visage.en_grand(ui.painter(), centre, rayon, &pose);
        self.rendu.push(depart.elapsed().as_secs_f64() * 1000.0);
        if t >= self.fin {
            self.envoi = None;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // L'image de la fenêtre, une fois peinte : la part du portrait, sa tête comprise.
        if let Some(envoi) = self.envoi.clone() {
            let ppp = ctx.pixels_per_point();
            let r = egui::Rect::from_min_max(
                centre + egui::vec2(-1.15 * rayon, -1.5 * rayon),
                centre + egui::vec2(1.15 * rayon, 1.05 * rayon),
            );
            let px = [r.min.x, r.min.y, r.max.x, r.max.y].map(|v| (v * ppp).round().max(0.0) as usize);
            let ms = (t * 1000.0).round() as u64;
            ctx.request_screenshot(move |image| {
                let _ = envoi.send((ms, image, px));
            });
        }
    }

    fn on_exit(&mut self, _: Option<&eframe::glow::Context>) {
        // Ce que le processeur a mis à préparer le portrait, image par image : la médiane, le 99e centile, le pire.
        self.rendu.sort_by(f64::total_cmp);
        if let Some(&pire) = self.rendu.last() {
            let centile = |p: f64| self.rendu[((self.rendu.len() - 1) as f64 * p) as usize];
            println!(
                "rendu du portrait, sur {} images : {:.2} ms en médiane, {:.2} au 99e centile, {:.2} au pire",
                self.rendu.len(),
                centile(0.5),
                centile(0.99),
                pire
            );
        }
        self.envoi = None;
        voix::faire_entendre(None);
        for e in self.ecrivains.drain(..) {
            let _ = e.join();
        }
    }
}

/// `eve --essai-oreille dossier [modèle]` : chaque WAV d'un dossier, transcrit par l'oreille du processeur comme Eve
/// transcrit une phrase (la fenêtre de la phrase, son amorce), le modèle chargé une fois — pour comparer ce qu'on
/// entend de deux voix, ou de deux façons de dire un texte. Une ligne par fichier : « nom (calcul) : texte ».
fn oreille_d_un_dossier(r: &Reglages, dossier: &Path, modele: Option<&String>) -> i32 {
    let modele = modele.map(std::path::PathBuf::from).unwrap_or_else(|| r.chemin_oreille());
    let parametres = whisper_rs::WhisperContextParameters::default();
    let contexte = match whisper_rs::WhisperContext::new_with_params(&modele, parametres) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("le modèle ne se charge pas : {e}");
            return 1;
        }
    };
    let Ok(mut etat) = contexte.create_state() else {
        eprintln!("Whisper ne démarre pas");
        return 1;
    };
    let mut wavs: Vec<std::path::PathBuf> = std::fs::read_dir(dossier)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "wav")).collect())
        .unwrap_or_default();
    wavs.sort();
    let (amorce, fils) = (r.vocabulaire_complet(), oreille::fils_de_calcul());
    for wav in wavs {
        let nom = wav.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match micro::lire_wav(&wav) {
            Ok(audio) => {
                let t0 = Instant::now();
                let texte = oreille::transcrire(&mut etat, &audio, &amorce, fils);
                println!("{nom} ({:.1} s) : {texte}", t0.elapsed().as_secs_f32());
            }
            Err(e) => println!("{nom} : illisible ({e})"),
        }
    }
    0
}

// ---- Les services en ligne du tableau de bord -------------------------------------------------------------------

/// `eve --essai-services [--nombres] [--suivre SECONDES]` : une lecture des services en ligne par leur fil, comme dans
/// la fenêtre — les trois pages d'état, puis les erreurs des ressources de Render avec la clé du dossier local
/// (`EVE_DOSSIER_LOCAL` ; `eve --cle render` la pose). Elle imprime le relevé ; `--nombres` : ses nombres et ses états
/// seulement, sans le nom d'une ressource ni une ligne de son journal (une sortie à montrer). Une lecture, une dizaine
/// de requêtes. `--suivre` : le suivi tourne encore tant de secondes, et chaque relevé nouveau, ou disparu, se dit en
/// une ligne — une clé retirée ou posée en marche se voit ainsi (une lecture toutes les 5 min).
pub fn services(args: &[String]) -> i32 {
    use crate::cles::{self, Service};
    use crate::services::{Services, Sorte};
    let usage = "eve --essai-services [--nombres] [--suivre SECONDES]";
    let (mut nombres, mut suivre) = (false, None);
    let mut mots = args.iter();
    while let Some(a) = mots.next() {
        match a.as_str() {
            "--nombres" => nombres = true,
            "--suivre" => match mots.next().and_then(|s| s.parse::<u64>().ok()) {
                Some(s) => suivre = Some(s),
                None => {
                    eprintln!("{usage} : --suivre veut un nombre de secondes");
                    return 2;
                }
            },
            _ => {
                eprintln!("{usage} : « {a} » ?");
                return 2;
            }
        }
    }
    let dossier = crate::reglages::dossier_local();
    if cles::lire(Service::Render).is_none() {
        eprintln!(
            "Aucune clé de Render ne se lit dans {} : sans elle, rien ne se lit (eve --cle render la pose).",
            dossier.display()
        );
        return 1;
    }
    let t0 = Instant::now();
    let suivi = Services::lancer();
    let r = loop {
        if let Some(r) = suivi.releve() {
            break r;
        }
        if t0.elapsed() > Duration::from_secs(120) {
            eprintln!("pas de relevé en deux minutes");
            return 1;
        }
        thread::sleep(Duration::from_millis(50));
    };
    let heure = |unix: i64| {
        crate::rappels::heure_locale(unix).and_then(|h| h.get(..16).map(str::to_string)).unwrap_or_else(|| "?".into())
    };
    println!("clé de Render : posée (dans {})", dossier.display());
    println!("lu en         : {}, à {}", crate::entreprise::secondes(t0.elapsed()), heure(r.lu_a));
    println!("pages d'état  :");
    for s in &r.statuts {
        match &s.etat {
            Err(e) => println!("  {:<7}: illisible — {e}", s.nom),
            Ok(e) => {
                println!(
                    "  {:<7}: {} · {} incident(s) en cours · {} composant(s) touché(s)",
                    s.nom,
                    e.niveau.libelle(),
                    e.incidents.len(),
                    e.composants.len()
                );
                if nombres {
                    continue;
                }
                for i in &e.incidents {
                    let depuis = heure(i.depuis);
                    println!(
                        "           incident : {} ({}, {}, depuis {depuis}) {}",
                        i.nom,
                        i.impact.libelle(),
                        i.statut,
                        i.lien
                    );
                }
                for (nom, n) in &e.composants {
                    println!("           composant : {nom} ({})", n.libelle());
                }
            }
        }
    }
    match &r.render {
        Err(e) => println!("Render        : ne se lit pas — {e}"),
        Ok(liste) => {
            println!("Render        : {} ressource(s)", liste.len());
            for (k, e) in liste.iter().enumerate() {
                let sorte = match e.sorte {
                    Sorte::Web => "service web",
                    Sorte::Base => "base",
                    Sorte::Autre => "autre service",
                };
                let nom = if nombres { format!("ressource {}", k + 1) } else { format!("« {} »", e.nom) };
                let plus = |p: bool| if p { " (au moins : il y en a davantage)" } else { "" };
                let derniere = |l: &crate::services::Ligne| match nombres {
                    true => format!("     la plus récente : {}", heure(l.quand)),
                    false => format!("     la plus récente : {} — {}", heure(l.quand), l.message),
                };
                println!(
                    "  {nom} ({sorte}) : {} erreur(s) en 24 h{}, {} dans la dernière heure",
                    e.nombre,
                    plus(e.plus),
                    e.derniere_heure
                );
                if let Some(l) = &e.derniere {
                    println!("{}", derniere(l));
                }
                if e.sorte == Sorte::Web {
                    let r5 = &e.reponses_5xx;
                    println!(
                        "     réponses HTTP 5xx hors du niveau « error » : {} en 24 h{}, {} dans la dernière heure",
                        r5.nombre,
                        plus(r5.plus),
                        r5.derniere_heure
                    );
                    if let Some(l) = &r5.derniere {
                        println!("{}", derniere(l));
                    }
                }
            }
        }
    }
    println!("résumé        : {}", r.resume());
    if let Some(secondes) = suivre {
        // Le suivi continue : un relevé nouveau (une autre heure de lecture), ou plus de relevé, se dit.
        let fin = Instant::now() + Duration::from_secs(secondes);
        let mut vu = Some(r.lu_a);
        while Instant::now() < fin {
            let maintenant = suivi.releve();
            if maintenant.as_ref().map(|r| r.lu_a) != vu {
                vu = maintenant.as_ref().map(|r| r.lu_a);
                let quand = crate::journal::date_et_heure();
                match &maintenant {
                    Some(r) => println!("{quand} : un relevé de {} — {}", heure(r.lu_a), r.resume()),
                    None => println!("{quand} : plus de relevé (la clé de Render est retirée, ou ne se lit plus)"),
                }
            }
            thread::sleep(Duration::from_millis(200));
        }
    }
    0
}

// ---- Stripe, pour le tableau de bord -------------------------------------------------------------------------------

/// `eve --essai-stripe` : une lecture du compte Stripe par son fil, comme dans la fenêtre, avec la clé du dossier
/// local (`EVE_DOSSIER_LOCAL` ; `eve --cle stripe` la pose). Elle imprime d'abord la sorte de la clé (jamais la clé),
/// les routes lues — toutes en GET — et les droits qu'elles demandent à une clé restreinte ; puis ce que le tableau de
/// bord en montrerait. Une lecture : sept requêtes, d'ordinaire.
pub fn stripe(args: &[String]) -> i32 {
    use crate::cles::{self, Service};
    use crate::stripe::{self, Issue, Route, Somme, Stripe};
    if !args.is_empty() {
        eprintln!("eve --essai-stripe : sans argument (la clé est celle du dossier local, EVE_DOSSIER_LOCAL)");
        return 2;
    }
    println!("dossier     : {}", crate::reglages::dossier_local().display());
    // La sorte de la clé, d'après son début seulement.
    let Some(sorte) = cles::lire(Service::Stripe).map(|c| {
        let sorte = match c.starts_with("rk_") {
            true => "restreinte",
            false => "secrète (tous les droits ; une clé restreinte suffit)",
        };
        format!("{sorte}, en mode {}", if c.contains("_live_") { "réel" } else { "d'essai" })
    }) else {
        eprintln!("aucune clé de Stripe ne se lit dans ce dossier local : eve --cle stripe la pose");
        return 1;
    };
    println!("clé         : {sorte}");
    for r in Route::TOUTES {
        let droits: Vec<&str> = r.droits().iter().map(|d| d.nom).collect();
        println!("route       : GET {} ({})", r.chemin(), droits.join(", "));
    }
    let droits: Vec<String> = stripe::DROITS.iter().map(|d| format!("{} ({})", d.nom, d.code)).collect();
    println!("droits      : {}, en lecture (« Read »)", droits.join(", "));
    println!("version     : Stripe-Version {}", stripe::VERSION);
    let t0 = Instant::now();
    let suivi = Stripe::lancer();
    let m = loop {
        match suivi.montants() {
            Some(Ok(m)) => break m,
            Some(Err(e)) => {
                eprintln!("pas de montants : {e}");
                return 1;
            }
            None if t0.elapsed() > Duration::from_secs(180) => {
                eprintln!("pas de montants en trois minutes");
                return 1;
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    };
    let argent = |x: f64| format!("{x:.2} {}", m.devise);
    let somme = |s: &Somme| {
        let pluriel = if s.nombre > 1 { "s" } else { "" };
        let plus = if s.plus { ", au moins" } else { "" };
        format!("{} ({} paiement{pluriel}{plus})", argent(s.montant), s.nombre)
    };
    println!("lu en       : {:.2} s", t0.elapsed().as_secs_f64());
    println!("disponible  : {}", argent(m.disponible));
    println!("en attente  : {}", argent(m.en_attente));
    println!("aujourd'hui : {}", somme(&m.aujourdhui));
    println!("ce mois-ci  : {}", somme(&m.ce_mois));
    match &m.abonnements {
        Some(a) => println!("abonnements : {} actifs, {} par mois", a.actifs, argent(a.mensuel)),
        None => println!("abonnements : non lus"),
    }
    match &m.virement {
        Some(v) if v.fait => println!("virement    : {}, arrivé le {}", argent(v.montant), v.arrivee),
        Some(v) => println!("virement    : {}, en route, attendu le {}", argent(v.montant), v.arrivee),
        None => println!("virement    : aucun"),
    }
    match &m.ouvertes {
        Some(o) => {
            let plus = if o.plus { " ou plus" } else { "" };
            println!("factures    : {}{plus} ouvertes, {} dus, {} échues", o.nombre, argent(o.du), o.echues);
        }
        None => println!("factures    : non lues"),
    }
    println!("paiements   : les {} derniers", m.paiements.len());
    for p in &m.paiements {
        let issue = match p.issue {
            Issue::Reussi => "réussi",
            Issue::Echoue => "échoué",
            Issue::Rembourse => "remboursé",
            Issue::EnAttente => "en attente",
        };
        let quand = crate::rappels::heure_locale(p.quand).unwrap_or_default();
        println!("  {quand}  {:>14}  {issue:<10}  {}", argent(p.montant), p.qui);
    }
    0
}

// ---- Le son de la voix, rendu sans le jouer -----------------------------------------------------------------------

/// `eve --essai-son <textes.txt> <dossier> [--bruts DOSSIER] [--id VOIX] [--douceur N]` : ce qu'Eve dirait de chaque
/// texte (une ligne du fichier chacun ; « \n » y fait une autre ligne, « \n\n » un autre paragraphe), rendu sans le
/// jouer — aucun son dans la pièce : le WAV de chaque phrase tel que le lecteur le recevrait (`phrase-TT-MM.wav`), le
/// tout de chaque texte, ses phrases à leur heure (`texte-TT.wav`), et ce qui s'y dit (`rendu.json`). Les WAV de Piper
/// se gardent (`--bruts`, d'office `<dossier>\bruts`) : un autre rendu les reprend, et seul le traitement change.
pub fn son(r: &Reglages, args: &[String]) -> i32 {
    let usage = "eve --essai-son <textes.txt> <dossier> [--bruts DOSSIER] [--id VOIX] [--douceur N]";
    let mut reglages = r.clone();
    let (mut positionnels, mut bruts) = (Vec::new(), None);
    let mut mots = args.iter();
    while let Some(a) = mots.next() {
        if !matches!(a.as_str(), "--bruts" | "--id" | "--douceur") {
            positionnels.push(a.clone());
            continue;
        }
        let Some(valeur) = mots.next() else {
            eprintln!("{usage} : {a} veut une valeur");
            return 2;
        };
        match a.as_str() {
            "--bruts" => bruts = Some(std::path::PathBuf::from(valeur)),
            "--id" => reglages.voix = valeur.clone(),
            _ => match valeur.parse() {
                Ok(n) => reglages.douceur = n,
                Err(_) => {
                    eprintln!("{usage} : --douceur veut un nombre");
                    return 2;
                }
            },
        }
    }
    let [textes, dossier] = positionnels.as_slice() else {
        eprintln!("{usage}");
        return 2;
    };
    let dossier = Path::new(dossier);
    let textes: Vec<String> = match std::fs::read_to_string(textes) {
        Ok(t) => t.lines().filter(|l| !l.trim().is_empty()).map(|l| l.replace("\\n", "\n")).collect(),
        Err(e) => {
            eprintln!("{textes} : {e}");
            return 1;
        }
    };
    let bruts = bruts.unwrap_or_else(|| dossier.join("bruts"));
    if let Err(e) = std::fs::create_dir_all(dossier) {
        eprintln!("{} : {e}", dossier.display());
        return 1;
    }
    println!(
        "voix : {} ; débit {} ; douceur {} ; {} texte(s)",
        reglages.voix,
        reglages.debit,
        reglages.douceur,
        textes.len()
    );
    let t0 = Instant::now();
    let rendus = match voix::rendre(&reglages, &textes, &bruts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("le rendu a échoué : {e}");
            return 1;
        }
    };
    let secondes = |d: Duration| (d.as_secs_f64() * 1000.0).round() / 1000.0;
    let mut resume = Vec::new();
    for (t, (texte, rendu)) in textes.iter().zip(&rendus).enumerate() {
        let tout = format!("texte-{:02}.wav", t + 1);
        let mut phrases = Vec::new();
        for (m, p) in rendu.phrases.iter().enumerate() {
            let nom = format!("phrase-{:02}-{:02}.wav", t + 1, m + 1);
            if let Err(e) = std::fs::write(dossier.join(&nom), &p.wav) {
                eprintln!("{nom} : {e}");
                return 1;
            }
            phrases.push(serde_json::json!({
                "fichier": nom,
                "texte": p.texte,
                "pause": secondes(p.pause),
                "debut": secondes(p.debut),
                "voix": [secondes(p.voix.0), secondes(p.voix.1)],
            }));
        }
        if let Err(e) = std::fs::write(dossier.join(&tout), &rendu.tout) {
            eprintln!("{tout} : {e}");
            return 1;
        }
        let dit: Vec<&str> = rendu.phrases.iter().map(|p| p.texte.as_str()).collect();
        println!("{tout} : {} phrase(s) — {}", rendu.phrases.len(), dit.join(" "));
        resume.push(serde_json::json!({ "texte": texte, "dit": dit.join(" "), "fichier": tout, "phrases": phrases }));
    }
    let json = serde_json::to_string_pretty(&resume).unwrap_or_default();
    if let Err(e) = std::fs::write(dossier.join("rendu.json"), json) {
        eprintln!("rendu.json : {e}");
        return 1;
    }
    println!("rendu en {:.1} s dans {}", t0.elapsed().as_secs_f32(), dossier.display());
    0
}
