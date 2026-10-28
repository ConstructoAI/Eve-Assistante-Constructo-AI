//! Eve — le programme du poste : la vitrine (une page servie en local, ouverte dans une fenêtre d'Edge), le micro,
//! l'oreille (whisper.cpp), la voix (Piper, les voix de Windows), la touche à tenir, et l'Adjointe (Claude Code), avec la
//! garde entre les deux. Voir `src/lib.rs` pour la partie pure, et le LISEZ-MOI de la trousse
//! (`crates/adjointe/trousse/Eve`).

mod ambiance;
mod cadre;
mod cartes;
mod centre;
mod cerveau;
mod chef;
mod cles;
mod connexion;
mod ecran;
mod entreprise;
mod epingles;
mod essais;
mod fenetre;
mod filtres;
mod groupe;
mod historique;
mod historique_ui;
mod https;
mod journal;
mod memoire;
mod meteo;
mod micro;
mod mise_a_jour;
mod musique;
mod natif;
mod notifications;
mod orbe;
mod oreille;
mod profils;
mod rappels;
mod reglages;
mod reseau;
mod services;
mod sons;
mod stripe;
mod touche;
mod veilleur;
mod voix;

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use chef::{Chef, Evt};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Ce qui ne fait que lire, ou ne touche qu'au programme : ni les réglages ni la vitrine ne s'ouvrent (ni ne se
    // créent), et l'instance unique ne compte pas. Les mesures et la version n'ouvrent pas non plus le journal.
    match args.first().map(String::as_str) {
        Some("--mesures") => std::process::exit(mesures(&args[1..])),
        Some("--version") => {
            println!("{}", mise_a_jour::ligne_de_version());
            std::process::exit(0);
        }
        Some("--mettre-a-jour") => std::process::exit(mise_a_jour::mettre_a_jour(&args[1..])),
        Some("--revenir") => std::process::exit(mise_a_jour::revenir(&args[1..])),
        Some(mise_a_jour::ARGUMENT_DU_GARDIEN) => std::process::exit(mise_a_jour::surveiller(&args[1..])),
        _ => {}
    }
    // Les profils (`--profils`, `--profil-ajouter`, `--profil`, `--profil-retirer`) et le cerveau (`--modele`,
    // `--effort`) : sur reglages.json du dossier local, que l'Eve en marche relit — lancés par Claude au milieu d'un
    // tour, ni le journal ni la vitrine ne s'ouvrent. Les clés d'API du tableau de bord (`--cle`) non plus.
    if let Some(code) = profils::commande(&args).or_else(|| reglages::commande(&args)).or_else(|| cles::commande(&args))
    {
        std::process::exit(code);
    }
    let reglages = reglages::charger();
    journal::ouvrir(&reglages::dossier_local());
    if let Some(raison) = reglages.illisible() {
        journal::ecrire(&format!(
            "{} ne se lit pas ({raison}) : les valeurs d'office servent, le fichier n'est pas touché.",
            reglages.chemin().display()
        ));
    }
    let code = match args.first().map(String::as_str) {
        Some("--voix") => essais::lister_voix(),
        Some("--essai-voix") => essais::voix(&reglages, &args[1..]),
        Some("--essai-oreille") => essais::oreille(&reglages, &args[1..]),
        Some("--essai-claude") => essais::claude(&reglages, &args[1..]),
        Some("--essai-micro") => essais::micro(&args[1..]),
        Some("--essai-orbe") => essais::orbe(&args[1..]),
        Some("--essai-cartes") => essais::cartes(&args[1..]),
        Some("--essai-musique") => essais::musique(&args[1..]),
        Some("--essai-reseau") => essais::reseau(&args[1..]),
        Some("--essai-meteo") => essais::meteo(&args[1..]),
        Some("--essai-panneaux") => essais::panneaux(&args[1..]),
        Some("--essai-ambiance") => essais::ambiance(&args[1..]),
        Some("--essai-historique") => essais::historique(&args[1..]),
        Some("--essai-constructo") => essais::constructo(&args[1..]),
        Some("--essai-cadre") => essais::cadre(&args[1..]),
        Some("--essai-theme") => essais::theme(&args[1..]),
        Some("--essai-pieces") => essais::pieces(&args[1..]),
        Some("--essai-profils") => essais::profils(&args[1..]),
        Some("--essai-portrait") => essais::portrait(&args[1..]),
        Some("--essai-services") => essais::services(&args[1..]),
        Some("--essai-stripe") => essais::stripe(&args[1..]),
        #[cfg(windows)]
        Some("--essai-notification") => notifications::essai(&crate::reglages::dossier_local()),
        Some("--essai-son") => essais::son(&reglages, &args[1..]),
        Some("--essai-tour") => essais::tour(&reglages, &args[1..]),
        Some("--aide" | "-h" | "--help") => {
            aide();
            0
        }
        _ => lancer(reglages, args.iter().any(|a| a == "--sans-fenetre"), args.iter().any(|a| a == "--native")),
    };
    std::process::exit(code);
}

fn lancer(mut reglages: reglages::Reglages, sans_fenetre: bool, native: bool) -> i32 {
    journal::proteger_la_console();
    // L'essai d'une version qui ne démarre pas (mise_a_jour.rs) : sans la variable, rien.
    if mise_a_jour::essai().as_deref() == Some("arret") {
        journal::ecrire(&format!(
            "essai : la version {} s'arrête dès son lancement (EVE_ESSAI_MISE_A_JOUR)",
            mise_a_jour::version()
        ));
        return 3;
    }
    // Le nom de l'entreprise, s'il n'est pas dans les réglages : celui de l'Adjointe (python, une seconde ou deux), avant
    // l'oreille, qui le prend dans son amorce. Il ne s'écrit pas dans reglages.json : un nom corrigé dans
    // config/entreprise.json vaut au prochain lancement.
    let dossier = reglages.dossier_adjointe();
    if reglages.entreprise.trim().is_empty()
        && let Some(nom) = dossier.as_deref().and_then(cerveau::nom_entreprise)
    {
        reglages.entreprise_lue = nom;
    }
    reglages.scan_lu = dossier.as_deref().and_then(cerveau::dossier_scan);
    // La garde reconnaît aussi les scripts de l'Adjointe par leur chemin absolu.
    if let Some(d) = &dossier {
        eve::garde::reconnaitre_dossier(&d.to_string_lossy());
    }
    journal::ecrire(&format!("Eve {} démarre ({}).", mise_a_jour::version(), reglages.qui));
    let (tx, rx) = mpsc::channel::<Evt>();
    let ecran = match ecran::demarrer(reglages.port, ecran::jeton_aleatoire(), tx.clone()) {
        Ok(e) => e,
        Err(e) if e.kind() == ErrorKind::AddrInUse => {
            journal::ecrire("Une Eve tourne déjà sur ce poste : sa fenêtre se rouvre.");
            if let Some(url) = ecran::adresse_existante() {
                ecran::ouvrir_fenetre(&url);
            }
            return 0;
        }
        Err(e) => {
            journal::ecrire(&format!("La vitrine ne s'ouvre pas sur le port {} : {e}", reglages.port));
            return 1;
        }
    };
    ecran.noter_session();
    let voix = voix::demarrer(&reglages, tx.clone());
    let oreille = oreille::demarrer(
        reglages.chemin_oreille(),
        reglages.chemin_oreille_carte(),
        reglages.chemin_oreille_eveil(),
        reglages.vocabulaire_complet(),
        tx.clone(),
    );
    touche::demarrer(reglages.touche().0, tx.clone());
    fenetre::demarrer_raccourci();
    // Le jeton d'Adjointe se lit dans un fil (python, quelques secondes) : la fenêtre s'ouvre sans l'attendre, et le
    // veilleur (la veille des courriels, le calendrier d'Entrepreneur Général) part avec lui. Chez l'Adjointe
    // seulement : un spécialiste n'a ni jeton ni veilleur, et son Claude Code part sans attendre.
    {
        let tx = tx.clone();
        let dossier = reglages.dossier_adjointe().filter(|_| !reglages.specialiste());
        let minutes = reglages.alertes.minutes_avant;
        thread::spawn(move || {
            let jeton = dossier.as_deref().and_then(cerveau::jeton_mcp);
            let _ = tx.send(Evt::Jeton(jeton.clone()));
            if dossier.is_some() {
                veilleur::demarrer(jeton, minutes, tx);
            }
        });
    }
    let battement = tx.clone();
    thread::spawn(move || {
        while battement.send(Evt::Tic).is_ok() {
            thread::sleep(Duration::from_millis(100));
        }
    });
    let avec_fenetre = reglages.fenetre && !sans_fenetre;
    if avec_fenetre && (native || reglages.fenetre_native) {
        // La fenêtre native veut le fil principal (winit) : le chef prend le sien, et la fenêtre se ferme avec lui.
        // Le titre de la fenêtre d'Edge en tête (fenetre.rs la retrouve ainsi), la marque après.
        let titre = format!("{} · Constructo AI", fenetre::titre_pour(reglages::nom_affiche()));
        let dossier = reglages.dossier_adjointe();
        let (abonnement, adresse, rapport) = (ecran.abonner(), ecran.adresse(), tx.clone());
        let chef = thread::spawn(move || {
            // Même si le chef panique, sa fenêtre ne reste pas ouverte sur un état figé.
            struct Fermer;
            impl Drop for Fermer {
                fn drop(&mut self) {
                    natif::fermer();
                }
            }
            let _fermer = Fermer;
            Chef::nouveau(reglages, ecran, voix, oreille, tx, true).mener(rx);
        });
        if !natif::lancer(abonnement, rapport, &titre, dossier) {
            ecran::ouvrir_fenetre(&adresse);
        }
        let _ = chef.join();
        return 0;
    }
    if avec_fenetre {
        ecran::ouvrir_fenetre(&ecran.adresse());
    } else {
        println!("Eve : {}", ecran.adresse());
    }
    Chef::nouveau(reglages, ecran, voix, oreille, tx, avec_fenetre).mener(rx);
    0
}

/// `eve --mesures [dossier ou journal…]` : les médianes de chaque étape des demandes et le coût des tours, relus dans
/// les journaux (`eve.1.log` puis `eve.log` d'un dossier). Sans rien : le dossier de cette Eve (`EVE_DOSSIER_LOCAL`),
/// sinon ceux de tous les spécialistes du poste, et celui d'Entrepreneur Général.
fn mesures(args: &[String]) -> i32 {
    let sources: Vec<PathBuf> =
        if args.is_empty() { dossiers_du_poste() } else { args.iter().map(PathBuf::from).collect() };
    if sources.is_empty() {
        eprintln!("Aucun journal d'Eve sur ce poste : eve --mesures <dossier ou eve.log>.");
        return 1;
    }
    let mut lus = 0;
    for source in &sources {
        let fichiers =
            if source.is_dir() { vec![source.join("eve.1.log"), source.join("eve.log")] } else { vec![source.clone()] };
        let textes: Vec<String> = fichiers
            .iter()
            .filter_map(|f| fs::read(f).ok())
            .map(|o| String::from_utf8_lossy(&o).into_owned())
            .collect();
        if textes.is_empty() {
            eprintln!("{} : aucun journal à lire.", source.display());
            continue;
        }
        lus += 1;
        println!("\n{}", source.display());
        let series = eve::mesures::lire_journal(&textes.join("\n"));
        if series.is_empty() {
            println!("  rien à mesurer");
        } else {
            print!("{}", eve::mesures::releve(&series));
        }
    }
    if lus > 0 { 0 } else { 1 }
}

/// Les dossiers locaux des Eve du poste qui ont un journal.
fn dossiers_du_poste() -> Vec<PathBuf> {
    if let Some(d) = std::env::var_os("EVE_DOSSIER_LOCAL").filter(|d| !d.is_empty()) {
        return vec![PathBuf::from(d)];
    }
    let Some(base) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else { return Vec::new() };
    let mut d: Vec<PathBuf> =
        fs::read_dir(base.join("Eve").join("specialistes")).into_iter().flatten().flatten().map(|e| e.path()).collect();
    d.sort();
    d.push(base.join("EntrepreneurGeneral").join("Eve"));
    d.into_iter().filter(|d| d.join("eve.log").is_file()).collect()
}

fn aide() {
    println!(
        "Eve — l'Adjointe de vive voix\n\
         \n\
         eve                          la fenêtre, le micro, la voix et l'Adjointe\n\
         eve --sans-fenetre           la même chose ; l'adresse de la page s'affiche\n\
         eve --native                 la fenêtre native, dans le programme, au lieu de la page dans Edge\n\
         eve --voix                   les voix du poste\n\
         eve --essai-voix [--id ID] texte    la voix dit un texte\n\
         eve --essai-oreille son.wav  l'oreille transcrit un fichier\n\
         eve --essai-micro [secondes] le micro écoute ; perd-il du son ? (30 s d'office)\n\
         eve --essai-cartes [dossier] [scène]  les cartes de la fenêtre native, les aperçus des fichiers du dossier\n\
         eve --essai-claude question  un tour avec l'Adjointe (toute écriture refusée d'office)\n\
         eve --essai-orbe [--etat NOM] l'orbe de la fenêtre native, seule ; --mesure : ce que coûte une image\n\
         eve --essai-musique [secondes] [dossier] [--marge]  la musique du centre de commande, et ce qu'en entend\n\
         \x20                            le détecteur de parole d'Eve (60 s d'office)\n\
         eve --essai-reseau [scène]   le réseau des actions, plein fenêtre (tout, demo, sujets, mesure…)\n\
         eve --essai-meteo [latitude longitude]  une lecture de la météo (Montréal d'office), sa tempête en hiver\n\
         eve --essai-panneaux [dossier] [heure]  les panneaux du centre de commande, sur des données inventées, à\n\
         \x20                            l'heure dite (--date, --tempete NEIGE,RAFALES, --captures DOSSIER) ;\n\
         \x20                            --comptabilite : l'argent vient d'une comptabilité inventée\n\
         \x20                            (--comptabilite-de DOSSIER : d'une autre, lue telle quelle)\n\
         eve --essai-ambiance [scène] l'ambiance du centre de commande : la saison, la fête, la nuit, la veille\n\
         \x20                            (tout, halloween, neige, noel, nuit, fete, veille, mesure…)\n\
         eve --essai-historique [dossier] [--captures DOSSIER]  le panneau de l'historique, sur des conversations\n\
         \x20                            inventées ; --captures : un parcours, une capture PNG à chaque étape\n\
         eve --essai-constructo [port] [--duree s] [--outils N] [--reveil s]  un faux Constructo AI sur le poste\n\
         \x20                            (47997 d'office), pour essayer la connexion d'Eve (EVE_CONSTRUCTO_MCP,\n\
         \x20                            EVE_SANS_NAVIGATEUR)\n\
         eve --essai-cadre [--captures DOSSIER]  la barre de titre d'Eve, mise à l'épreuve comme Windows le ferait\n\
         eve --essai-theme [dossier]  la fenêtre native dans le thème « Constructo AI », sur un état inventé\n\
         \x20                            (--captures DOSSIER : une capture par scène)\n\
         eve --essai-pieces [--captures DOSSIER] [--sans-selecteur]  les pièces jointes de la fenêtre native, sur des\n\
         \x20                            fichiers inventés (le dossier local de l'essai : EVE_DOSSIER_LOCAL)\n\
         eve --essai-profils [--captures DOSSIER]  le menu des profils de la barre et leur rubrique des réglages,\n\
         \x20                            cliqués sans la souris, sur des profils inventés\n\
         eve --essai-portrait <dossier> [--captures DOSSIER]  le portrait d'Eve d'un dossier local dans toutes ses\n\
         \x20                            poses : les clignements, les regards, les bouches, et leurs mélanges\n\
         eve --essai-services [--nombres] [--suivre s]  une lecture des services en ligne du tableau de bord : les\n\
         \x20                            pages d'état de Claude, de GitHub et de Render, les erreurs de Render (la\n\
         \x20                            clé : eve --cle render) ; --nombres : sans nom de ressource ni ligne de\n\
         \x20                            journal ; --suivre : le suivi tourne encore tant de secondes\n\
         eve --essai-stripe           une lecture du compte Stripe (la clé du dossier local : eve --cle stripe),\n\
         \x20                            ce que le tableau de bord en montre ; les droits d'une clé restreinte\n\
         eve --essai-son textes.txt dossier [--bruts DOSSIER] [--id VOIX] [--douceur N]  ce qu'Eve dirait de chaque\n\
         \x20                            texte, rendu sans le jouer : les WAV de ses phrases et leur tout\n\
         eve --essai-tour <dossier> [--fin s] [--oreille s] [--modele M]  le tour de parole rejoué sur des sons\n\
         \x20                            inventés (WAV et manifestes) : l'écoute du micro, l'oreille, ce qui part\n\
         \x20                            à Claude et quand, avant et après la fin du tour qui attend la suite\n\
         eve --mesures [dossier…]     le temps de chaque étape des demandes et le coût des tours, lus au journal\n\
         eve --profils                les profils de cette Eve (EVE_DOSSIER_LOCAL), le profil choisi marqué\n\
         eve --profil-ajouter \"nom\" \"dossier\"  un profil de plus : un dossier qui porte son CLAUDE.md\n\
         eve --profil [\"nom\"]         le profil choisi, où Eve passe entre deux demandes (rien : celui d'office)\n\
         eve --profil-retirer \"nom\"   le retire de la liste ; son dossier reste tel quel\n\
         eve --modele [sonnet|opus [moyen|max]]  le modèle de Claude et son effort (rien : ceux de cette Eve) ;\n\
         \x20                            Eve repart avec eux entre deux demandes, la conversation reprise\n\
         eve --effort moyen|max       l'effort seul\n\
         eve --prononcer [\"mot\" [\"ce que la voix lit\"]]  une prononciation à soi (« Granby » : « Granbi ») ;\n\
         \x20                            rien : celles de cette Eve ; le mot seul : la retire\n\
         eve --cle [render|stripe [--retirer]]  la clé d'API d'un service que montre le tableau de bord, collée\n\
         \x20                            sur l'entrée standard (chiffrée dans le dossier local) ; rien : ce qui est posé\n\
         eve --version                la version de ce programme et l'empreinte de sa source (son commit)\n\
         eve --mettre-a-jour eve.exe  installe ce programme-là (une version commitée, construite par Compiler.bat) :\n\
         \x20                            l'Eve qui tourne (EVE_DOSSIER_LOCAL) redémarre dedans à la fin de sa\n\
         \x20                            réponse, et revient d'elle-même à la version d'avant s'il ne démarre pas\n\
         eve --revenir                revient à la version précédente, par le même chemin\n\
         \n\
         Réglages : reglages.json du dossier local (EVE_DOSSIER_LOCAL, sinon\n\
         %LOCALAPPDATA%\\EntrepreneurGeneral\\Eve) ; journal : eve.log, à côté ; les versions gardées : versions\\, à\n\
         côté du programme."
    );
}
