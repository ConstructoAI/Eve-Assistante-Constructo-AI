//! Les cartes des réponses d'Eve, dans la fenêtre native (le lot 6.3). Après ses trois tirets, Claude peut écrire un
//! bloc « ```carte » qui ne contient qu'un objet JSON : un agenda, un tableau, un brouillon d'Outlook, l'aperçu d'un
//! fichier du poste. La page d'Edge les dessinait (`page\eve.html` : `carteAgenda`, `carteTableau`, `carteBrouillon`,
//! `carteApercu`) ; la fenêtre native les dessine ici, avec le même contenu et les mêmes règles, dans son propre style.
//! Leurs formats : `docs\fenetre-native.md`, § 2.6 et 2.7. Le découpage du texte en blocs est celui de `natif.rs`.
//! Une carte se montre tout entière : c'est la conversation qui défile, jamais la carte (un tableau ne défile qu'en
//! largeur, quand ses colonnes ne tiennent pas).
//!
//! Claude dessine aussi, comme dans claude.ai : un bloc « ```svg » de sa réponse devient un dessin (`Cartes::dessin`),
//! sur une feuille blanche, de toute la largeur de la conversation, qu'on ouvre en grand ou qu'on enregistre (PNG ou
//! SVG). Le dessin se fait dans le fil des aperçus, par resvg, à la largeur où il s'affiche.
//!
//! Le module ne parle pas au chef : un clic rend un [`Geste`] (l'action et sa valeur, celles de la page), que la
//! fenêtre lui transmet. Ce qui dure d'une image à l'autre vit dans [`Cartes`] : les cartes déjà lues (le JSON ne se
//! relit pas à chaque image, un tableau ne se remesure pas), les aperçus et les dessins avec leurs textures, le fil qui
//! lit les fichiers et dessine les SVG.
//!
//! L'aperçu lit un fichier : jamais dans l'image, toujours dans ce fil ; 25 Mo au plus ; et seulement un fichier local,
//! sous le dossier de travail ou le dossier local d'Eve. C'est le correctif n° 5 de l'analyse : la page ouvrait
//! n'importe quel chemin, un chemin réseau compris, et Windows s'authentifie alors auprès du serveur qui le sert. Le
//! chemin se juge deux fois : sur sa forme d'abord, sans toucher au disque (un chemin réseau est refusé avant toute
//! ouverture) ; puis, dans le fil, sur ce qu'il désigne vraiment, les jonctions et les liens résolus par Windows.

use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Read;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::SystemTime;

use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Galley, Margin, Pos2, Rect, RichText, Sense, Stroke,
    StrokeKind, Vec2, pos2, vec2,
};
use resvg::usvg::fontdb;
use serde_json::Value;

use crate::natif::{self, couleurs, icone, icones, melange, semi};

/// Un aperçu : 25 Mo au plus, comme celui de la page (`ecran.rs`).
const APERCU_MAX: u64 = 25 * 1024 * 1024;
/// La hauteur d'une image dans sa carte, au moins (`.ap-image`, `.ap-cadre` dans la page) : une conversation plus haute
/// en donne davantage ([`haut_max`]).
const APERCU_HAUT: f32 = 360.0;
/// Le grand côté d'une texture d'aperçu : de quoi remplir une carte sur un écran à 200 %, sans charger une vieille
/// carte graphique.
const TEXTURE_MAX: usize = 2048;
/// Un dessin : 512 Ko de SVG au plus. Claude en écrit quelques-uns, pendant qu'on attend ; au-delà, ce n'est plus un
/// dessin de réponse.
const DESSIN_MAX: usize = 512 * 1024;
/// Un dessin se rend à la largeur où il s'affiche, arrondie au palier du dessus (en pixels) : une fenêtre qu'on étire
/// ne le redessine qu'à chaque palier franchi.
const PALIER: u32 = 256;
/// La police d'un texte de dessin qui n'en nomme pas : celle de la fenêtre (resvg prendrait Times New Roman).
pub const POLICE_DES_DESSINS: &str = "Segoe UI";
/// Le grand côté d'un dessin enregistré en PNG : de quoi l'imprimer ou l'envoyer.
const PNG_COTE: usize = 2048;
/// Tant qu'un aperçu est à l'écran, son fichier se revérifie à ce rythme : Claude réécrit souvent le même plan, et la
/// carte montre alors la nouvelle version (la page gardait l'ancienne).
const REVERIFIER: f64 = 2.0;
/// Un aperçu qu'on n'a pas vu depuis ce temps s'oublie, sa texture avec lui ; il se relit s'il revient à l'écran.
const OUBLIER: f64 = 300.0;
/// Une carte qui n'est plus dessinée s'oublie après ce temps : elle a quitté le fil.
const OUBLIER_CARTE: f64 = 30.0;
/// Le carré de l'icône d'une carte (K2).
const TUILE: f32 = 24.0;
/// Les marges des cellules d'un tableau : à droite d'une colonne (sauf la dernière), en haut et en bas d'une rangée.
const MARGE_COLONNE: f32 = 14.0;
const MARGE_RANGEE: f32 = 6.0;

/// Les couleurs des cartes : celles du thème sombre de la fenêtre, ou celles du thème clair « Constructo AI »
/// ([`natif::theme`]) — une carte prend celles du `Ui` qui la porte ([`teintes`]).
struct Teintes {
    /// Le fond d'une carte, un peu plus clair que la bulle d'Eve qui la porte en sombre (comme dans la page), un peu
    /// plus soutenu que la bulle blanche en clair ; son bord ; un creux (l'attente d'un aperçu, ses avis).
    fond: Color32,
    bord: Color32,
    creux: Color32,
    /// Le texte, le secondaire, le discret ; les filets, le filet fort ; l'accent ; une rangée survolée ; le fond d'un
    /// bloc brut ; l'ombre d'une page de PDF.
    encre: Color32,
    encre_2: Color32,
    encre_3: Color32,
    filet: Color32,
    filet_fort: Color32,
    accent: Color32,
    survol: Color32,
    brut: Color32,
    ombre: Color32,
    /// L'en-tête d'un tableau : son bandeau (aucun en sombre ; marine en clair, comme les tableaux de l'assistant de
    /// l'ERP, `aiMarkdown.ts`) et son encre.
    entete_fond: Option<Color32>,
    entete_encre: Color32,
    /// Les sortes d'événements de l'agenda : les couleurs du calendrier d'Entrepreneur Général (la page, dans ses deux
    /// thèmes).
    s_confirme: Color32,
    s_attente: Color32,
    s_appel: Color32,
    s_ferie: Color32,
}

const SOMBRES: Teintes = Teintes {
    fond: Color32::from_rgb(0x21, 0x2E, 0x3E),
    bord: Color32::from_rgb(0x31, 0x41, 0x57),
    creux: Color32::from_rgb(0x1A, 0x25, 0x33),
    encre: couleurs::ENCRE,
    encre_2: couleurs::ENCRE_2,
    encre_3: couleurs::ENCRE_3,
    filet: couleurs::FILET,
    filet_fort: couleurs::FILET_FORT,
    accent: couleurs::ACCENT,
    survol: Color32::from_rgba_premultiplied(7, 7, 7, 7),
    brut: couleurs::FOND,
    ombre: Color32::from_rgba_premultiplied(0, 0, 0, 120),
    entete_fond: None,
    entete_encre: couleurs::ENCRE_3,
    s_confirme: Color32::from_rgb(0x64, 0xB4, 0xE0),
    s_attente: Color32::from_rgb(0xE0, 0x91, 0x6A),
    s_appel: Color32::from_rgb(0xAA, 0x9B, 0xD4),
    s_ferie: Color32::from_rgb(0x6E, 0x7D, 0x89),
};

/// En clair : les couleurs de l'Eve de Constructo AI dans son thème clair (`jarvis.html`, `.carte`), le texte des
/// tableaux de l'assistant de l'ERP (#334155) et son en-tête marine.
const CLAIRES: Teintes = Teintes {
    fond: Color32::from_rgb(0xF5, 0xF8, 0xFC),
    bord: Color32::from_rgb(0xE2, 0xE8, 0xF0),
    creux: Color32::from_rgb(0xEC, 0xF1, 0xF7),
    encre: Color32::from_rgb(0x33, 0x41, 0x55),
    encre_2: Color32::from_rgb(0x47, 0x55, 0x69),
    encre_3: Color32::from_rgb(0x64, 0x74, 0x8B),
    filet: Color32::from_rgb(0xE2, 0xE8, 0xF0),
    filet_fort: Color32::from_rgb(0xC2, 0xCE, 0xDD),
    accent: Color32::from_rgb(0x00, 0x78, 0xD4),
    survol: Color32::from_rgba_premultiplied(0, 0, 0, 8),
    brut: Color32::from_rgb(0xF1, 0xF5, 0xF9),
    ombre: Color32::from_rgba_premultiplied(0, 0, 0, 40),
    entete_fond: Some(Color32::from_rgb(0x00, 0x20, 0x50)),
    entete_encre: Color32::WHITE,
    s_confirme: Color32::from_rgb(0x0E, 0x5C, 0x8A),
    s_attente: Color32::from_rgb(0xA8, 0x48, 0x1A),
    s_appel: Color32::from_rgb(0x6B, 0x5B, 0x96),
    s_ferie: Color32::from_rgb(0x8A, 0x97, 0xA2),
};

/// Les couleurs des cartes dans ce `Ui` : claires dans un `Ui` aux visuels clairs.
fn teintes(ui: &egui::Ui) -> &'static Teintes {
    if ui.visuals().dark_mode { &SOMBRES } else { &CLAIRES }
}
/// La vignette d'une page HTML se dessine en clair, comme une page : l'encre et les filets du thème clair de la page.
const PAGE_ENCRE: Color32 = Color32::from_rgb(0x18, 0x21, 0x2B);
const PAGE_ENCRE_2: Color32 = Color32::from_rgb(0x4B, 0x57, 0x65);
const PAGE_ENCRE_3: Color32 = Color32::from_rgb(0x5F, 0x6B, 0x79);
const PAGE_BARRE: Color32 = Color32::from_rgb(0xEE, 0xF1, 0xF4);
const PAGE_FILET: Color32 = Color32::from_rgb(0xDC, 0xE1, 0xE7);

/// Les icônes des cartes qui ne sont pas déjà dans `natif::icones` (Segoe Fluent Icons) ; le tableau de bord prend
/// les mêmes pour les cartes qu'on y épingle.
pub(crate) mod pictos {
    pub const TABLEAU: char = '\u{E80A}';
    pub const IMAGE: char = '\u{EB9F}';
    pub const PDF: char = '\u{EA90}';
    pub const OUVRIR: char = '\u{E8A7}';
    pub const BOUCLIER: char = '\u{E730}';
    pub const INFO: char = '\u{E946}';
    /// La palette d'un dessin, la disquette d'« Enregistrer ».
    pub const DESSIN: char = '\u{E790}';
    pub const ENREGISTRER: char = '\u{E74E}';
    /// L'épingle d'une carte épinglée au tableau de bord.
    pub const EPINGLE: char = '\u{E718}';
}

// ---- Ce que rend une carte, et ce qui dure ------------------------------------------------------------------------

/// Ce qu'une carte demande au chef quand on clique : une action et sa valeur, comme un geste de la page
/// (« ouvrir_brouillon » et l'id du brouillon, « ouvrir_fichier » et le chemin — voir docs\fenetre-native.md, § 1.6).
#[derive(Debug, Clone, PartialEq)]
pub struct Geste {
    pub action: &'static str,
    pub valeur: Value,
}

/// Ce qui dure d'une image à l'autre : les cartes déjà lues, les aperçus et les dessins avec leurs textures, les
/// lectures en cours.
pub struct Cartes {
    /// Les cartes déjà analysées, par l'empreinte de leur JSON.
    modeles: HashMap<u64, Modele>,
    /// Les aperçus, par le chemin qu'écrit la carte.
    apercus: HashMap<String, Apercu>,
    /// Les dessins, par l'empreinte de leur SVG.
    dessins: HashMap<u64, Dessin>,
    /// Ce qu'a donné le dernier « Enregistrer » d'un dessin (l'empreinte du dessin, et la phrase à montrer sous lui) :
    /// la boîte de dialogue tient dans son fil, qui le dépose ici.
    enregistre: Arc<Mutex<Option<(u64, String)>>>,
    /// Les dossiers d'où un aperçu se lit, normalisés : le premier jugement, sur la forme du chemin.
    racines: Vec<Chemin>,
    /// Le dossier local du spécialiste, toujours parmi eux.
    local: PathBuf,
    lecture: Lecture,
    /// L'image où le ménage s'est fait : une fois par image, pas à chaque carte.
    menage: u64,
    /// Combien de fois chaque carte s'est dessinée dans cette image : la même carte deux fois dans le fil garde deux
    /// états (son défilement) d'une image à l'autre.
    occurrences: HashMap<u64, u32>,
}

impl Cartes {
    /// `travail` : le dossier de travail ; `local` : le dossier local du spécialiste (les seuls d'où un aperçu se lit).
    pub fn nouvelles(travail: Option<PathBuf>, local: PathBuf) -> Cartes {
        let mut cartes = Cartes {
            modeles: HashMap::new(),
            apercus: HashMap::new(),
            dessins: HashMap::new(),
            enregistre: Arc::default(),
            racines: Vec::new(),
            local,
            lecture: Lecture { racines: Arc::default(), envoi: None, rendus: Arc::default(), dessins: Arc::default() },
            menage: u64::MAX,
            occurrences: HashMap::new(),
        };
        cartes.permettre(travail.into_iter().collect());
        cartes
    }

    /// Les dossiers de travail d'où un aperçu se lit, avec le dossier local : celui d'Eve, et ceux de ses profils que
    /// publie le chef ([`crate::profils`]) — un plan que Claude dessine dans le dossier du profil où il travaille
    /// s'affiche. Le fil des lectures repart avec eux ; un aperçu refusé se rejuge à sa prochaine vérification.
    pub fn permettre(&mut self, travail: Vec<PathBuf>) {
        let brutes: Vec<PathBuf> = travail.into_iter().chain([self.local.clone()]).collect();
        // Une racine qui n'est pas un dossier local complet (un dossier sur le réseau) n'ouvre rien.
        self.racines = brutes.iter().filter_map(|r| normaliser(&r.to_string_lossy()).ok()).collect();
        self.lecture.racines = Arc::new(brutes);
        self.lecture.envoi = None;
    }

    /// Dessine la carte `json` dans la bulle d'Eve. `en_cours` : Eve l'écrit encore (un JSON incomplet dit alors
    /// « Carte en préparation… ») ; `opacite` : 1, ou 0,55 pour une ligne d'une séance précédente. Rend le geste
    /// qu'un clic demande, s'il y en a un.
    pub fn dessiner(&mut self, ui: &mut egui::Ui, json: &str, en_cours: bool, opacite: f32) -> Option<Geste> {
        let temps = ui.input(|i| i.time);
        self.ranger(ui.ctx(), temps);
        let cle = empreinte_du_texte(json);
        let Cartes { modeles, apercus, racines, lecture, occurrences, .. } = self;
        let modele = modeles.entry(cle).or_insert_with(|| Modele::lire(json));
        modele.vu = temps;
        let fois = occurrences.entry(cle).or_insert(0);
        *fois += 1;
        let id = ui.scope_id().with(("carte", cle, *fois));
        let mut geste = None;
        // Une colonne à elle, quelle que soit la disposition de la bulle qui la porte.
        ui.vertical(|ui| {
            ui.multiply_opacity(opacite);
            ui.add_space(2.0);
            let Modele { carte, mesures, mises_en_page, .. } = modele;
            match carte {
                None if en_cours => en_preparation(ui, "Carte en préparation…"),
                None => bloc_brut(ui, json, id),
                // L'agenda, le tableau et le brouillon se copient d'un clic dans leur tête ([`en_texte`]).
                Some(Carte::Agenda(a)) => {
                    cadre(ui, icones::CALENDRIER, &a.titre, "", Some(json), |ui| agenda(ui, a, id));
                }
                Some(Carte::Tableau(t)) => {
                    let n = t.lignes.len() as u64;
                    let meta = if n == 0 { String::new() } else { nombre_de(n, "ligne") };
                    let contenu = |ui: &mut egui::Ui| tableau(ui, t, mesures, mises_en_page, id);
                    cadre(ui, pictos::TABLEAU, &t.titre, &meta, Some(json), contenu);
                }
                Some(Carte::Brouillon(b)) => {
                    geste = cadre(ui, icones::COURRIEL, &b.titre, "", Some(json), |ui| brouillon(ui, b));
                }
                Some(Carte::Apercu(c)) => geste = apercu(ui, c, apercus, racines, lecture, temps, id),
                Some(Carte::Rappel { a, texte }) => {
                    cadre(ui, icones::CLOCHE, "Rappel", &quand_du_rappel(a), None, |ui| {
                        let encre = teintes(ui).encre;
                        ui.add(egui::Label::new(RichText::new(texte.as_str()).size(13.5).color(encre)).wrap());
                    });
                }
                Some(Carte::Epingle { id, titre, retiree }) => {
                    geste = cadre(ui, pictos::EPINGLE, titre, "Tableau de bord", None, |ui| epingle(ui, id, *retiree));
                }
            }
            ui.add_space(2.0);
        });
        geste
    }

    /// Dessine le dessin `svg` (un bloc « ```svg » de la réponse) dans la bulle d'Eve. `fini` : sa clôture est lue (tant
    /// qu'Eve l'écrit, « Dessin en préparation… ») ; `opacite` : comme pour une carte. Rend le geste qu'un clic demande.
    pub fn dessin(&mut self, ui: &mut egui::Ui, svg: &str, fini: bool, opacite: f32) -> Option<Geste> {
        let temps = ui.input(|i| i.time);
        self.ranger(ui.ctx(), temps);
        let cle = empreinte_du_texte(svg);
        let fois = self.occurrences.entry(cle).or_insert(0);
        *fois += 1;
        let id = ui.scope_id().with(("dessin", cle, *fois));
        let Cartes { dessins, lecture, local, enregistre, .. } = self;
        let mut geste = None;
        ui.vertical(|ui| {
            ui.multiply_opacity(opacite);
            ui.add_space(2.0);
            if fini {
                let d = dessins.entry(cle).or_insert_with(|| Dessin::nouveau(svg, temps));
                d.vu = temps;
                geste = carte_dessin(ui, cle, d, lecture, local, enregistre, id, true);
            } else {
                en_preparation(ui, "Dessin en préparation…");
            }
            ui.add_space(2.0);
        });
        geste
    }

    /// Le tableau d'une carte `json` (ses `colonnes`, ses `lignes`, son `pied`) sans son cadre ni sa tête : dans une
    /// carte du tableau de bord, qui a les siens.
    pub fn tableau_seul(&mut self, ui: &mut egui::Ui, json: &str) {
        let temps = ui.input(|i| i.time);
        self.ranger(ui.ctx(), temps);
        let cle = empreinte_du_texte(json);
        let modele = self.modeles.entry(cle).or_insert_with(|| Modele {
            carte: serde_json::from_str::<Value>(json)
                .ok()
                .and_then(|v| v.as_object().map(|d| Carte::Tableau(lire_tableau(d)))),
            vu: 0.0,
            mesures: None,
            mises_en_page: Vec::new(),
        });
        modele.vu = temps;
        let id = ui.scope_id().with(("tableau-seul", cle));
        let Modele { carte, mesures, mises_en_page, .. } = modele;
        match carte {
            Some(Carte::Tableau(t)) => tableau(ui, t, mesures, mises_en_page, id),
            _ => vide(ui, "Tableau illisible."),
        }
    }

    /// Le dessin `svg` sans son cadre : dans une carte du tableau de bord. Rend le geste qu'un clic demande.
    pub fn dessin_seul(&mut self, ui: &mut egui::Ui, svg: &str) -> Option<Geste> {
        let temps = ui.input(|i| i.time);
        self.ranger(ui.ctx(), temps);
        let cle = empreinte_du_texte(svg);
        let id = ui.scope_id().with(("dessin-seul", cle));
        let Cartes { dessins, lecture, local, enregistre, .. } = self;
        let d = dessins.entry(cle).or_insert_with(|| Dessin::nouveau(svg, temps));
        d.vu = temps;
        carte_dessin(ui, cle, d, lecture, local, enregistre, id, false)
    }

    /// Une fois par image : ce que le fil de lecture a rendu devient des textures, et ce qu'on ne voit plus s'oublie.
    fn ranger(&mut self, ctx: &egui::Context, temps: f64) {
        let passe = ctx.cumulative_pass_nr();
        if passe == self.menage {
            return;
        }
        self.menage = passe;
        self.occurrences.clear();
        let rendus = std::mem::take(&mut *self.lecture.rendus.lock().unwrap_or_else(|e| e.into_inner()));
        for (cle, lu) in rendus {
            if let Some(a) = self.apercus.get_mut(&cle) {
                a.recevoir(ctx, &cle, lu);
            }
        }
        let dessins = std::mem::take(&mut *self.lecture.dessins.lock().unwrap_or_else(|e| e.into_inner()));
        for (cle, rendu) in dessins {
            if let Some(d) = self.dessins.get_mut(&cle) {
                d.recevoir(ctx, cle, rendu);
            }
        }
        self.modeles.retain(|_, m| temps - m.vu < OUBLIER_CARTE);
        self.apercus.retain(|_, a| a.en_lecture || temps - a.vu < OUBLIER);
        self.dessins.retain(|_, d| d.en_lecture || temps - d.vu < OUBLIER);
    }
}

/// Une carte lue, et ce que son dessin garde d'une image à l'autre.
struct Modele {
    /// `None` : un JSON qui ne se lit pas (faux, ou pas encore fini), ou une sorte de carte inconnue.
    carte: Option<Carte>,
    vu: f64,
    /// Un tableau : ses colonnes mesurées, et sa mise en page pour les dernières largeurs où il s'est dessiné.
    mesures: Option<Mesures>,
    mises_en_page: Vec<MiseEnPage>,
}

impl Modele {
    fn lire(json: &str) -> Modele {
        Modele { carte: lire_carte(json), vu: 0.0, mesures: None, mises_en_page: Vec::new() }
    }
}

fn empreinte_du_texte(t: &str) -> u64 {
    let mut h = DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

// ---- La lecture des cartes (le contenu de la page) -----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Carte {
    Agenda(Agenda),
    Tableau(Tableau),
    Brouillon(Brouillon),
    Apercu(CarteApercu),
    /// Un rappel que Claude a posé (le chef l'a mis dans `rappels.json`) : son heure, « AAAA-MM-JJ HH:MM », et son texte.
    Rappel {
        a: String,
        texte: String,
    },
    /// Une carte que Claude a épinglée au tableau de bord (le chef l'a mise dans `epingles.json`), ou retirée : son id,
    /// son titre. Elle se dessine là-bas ; ici, ce qui s'est fait.
    Epingle {
        id: String,
        titre: String,
        retiree: bool,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct Agenda {
    titre: String,
    jours: Vec<Jour>,
}

#[derive(Debug, Clone, PartialEq)]
struct Jour {
    jour: String,
    evenements: Vec<Evenement>,
}

#[derive(Debug, Clone, PartialEq)]
struct Evenement {
    heure: String,
    titre: String,
    detail: String,
    sorte: Option<SorteEv>,
}

/// Les sortes d'événements du calendrier d'Entrepreneur Général (`SORTES` dans la page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SorteEv {
    Confirme,
    Attente,
    Appel,
    Echeance,
    Gros,
    Ferie,
}

impl SorteEv {
    fn lire(nom: &str) -> Option<SorteEv> {
        Some(match nom {
            "confirme" => SorteEv::Confirme,
            "attente" => SorteEv::Attente,
            "appel" => SorteEv::Appel,
            "echeance" => SorteEv::Echeance,
            "gros" => SorteEv::Gros,
            "ferie" => SorteEv::Ferie,
            _ => return None,
        })
    }

    fn nom(self) -> &'static str {
        match self {
            SorteEv::Confirme => "Confirmé",
            SorteEv::Attente => "À confirmer",
            SorteEv::Appel => "Appel à faire",
            SorteEv::Echeance => "Échéance",
            SorteEv::Gros => "Jalon",
            SorteEv::Ferie => "Férié",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Tableau {
    titre: String,
    colonnes: Vec<String>,
    lignes: Vec<Vec<String>>,
    pieds: Vec<Vec<String>>,
    /// Le nombre de colonnes : celui de la plus longue rangée.
    n: usize,
    /// Les colonnes de nombres, alignées à droite, en-tête compris.
    a_droite: Vec<bool>,
}

impl Tableau {
    /// Une cellule s'aligne à droite si sa colonne est une colonne de nombres, ou si elle ressemble elle-même à un
    /// nombre ou à un montant.
    fn droite(&self, k: usize, texte: &str) -> bool {
        self.a_droite.get(k).copied().unwrap_or(false) || ressemble_a_un_nombre(texte)
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Brouillon {
    titre: String,
    a: String,
    objet: String,
    extrait: String,
    id: String,
}

#[derive(Debug, Clone, PartialEq)]
struct CarteApercu {
    titre: String,
    /// Le chemin tel que la carte l'écrit, et son nom de fichier.
    chemin: String,
    nom: String,
    /// `None` : un genre de fichier qui ne s'affiche pas en aperçu.
    sorte: Option<SorteFichier>,
}

/// Les fichiers qui s'affichent en aperçu, comme dans la page : png, jpg, jpeg, gif, webp, svg, pdf, html, htm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SorteFichier {
    Image,
    Svg,
    Pdf,
    Page,
}

/// La carte d'un JSON, comme la page la lit ; `None` pour un JSON faux, ce qui n'est pas un objet, une sorte inconnue.
/// Un JSON presque juste (une virgule de trop, des guillemets simples) se lit quand même, en JSON5 : la page l'aurait
/// montré en bloc brut.
fn lire_carte(json: &str) -> Option<Carte> {
    let json = json.trim();
    let v: Value = serde_json::from_str(json).or_else(|_| json5::from_str(json)).ok()?;
    let d = v.as_object()?;
    let champ = |cle: &str| texte(d.get(cle));
    Some(match d.get("type").and_then(Value::as_str)? {
        "agenda" => Carte::Agenda(Agenda { titre: ou(champ("titre"), "Agenda"), jours: lire_jours(d) }),
        "tableau" => Carte::Tableau(lire_tableau(d)),
        "brouillon" => {
            let a = match d.get("a") {
                Some(Value::Array(l)) => l.iter().map(|x| texte(Some(x))).collect::<Vec<_>>().join(", "),
                x => texte(x),
            };
            Carte::Brouillon(Brouillon {
                titre: ou(champ("titre"), "Brouillon prêt dans Outlook"),
                a,
                objet: champ("objet"),
                extrait: champ("extrait"),
                id: champ("id"),
            })
        }
        "apercu" => {
            let chemin = champ("chemin");
            let nom = chemin.rsplit(['\\', '/']).next().unwrap_or("").to_string();
            let titre = ou(champ("titre"), if nom.is_empty() { "Aperçu" } else { &nom });
            Carte::Apercu(CarteApercu { titre, sorte: sorte_de_fichier(&nom), chemin, nom })
        }
        "rappel" => Carte::Rappel { a: champ("a"), texte: champ("texte") },
        "epingle" => {
            let (titre, donne) = (champ("titre"), champ("id"));
            let id = crate::natif::theme::cle_de(if donne.trim().is_empty() { &titre } else { &donne });
            let retiree = d.get("retirer").and_then(Value::as_bool) == Some(true);
            Carte::Epingle { titre: ou(titre, if retiree { "Carte" } else { "Carte sans titre" }), id, retiree }
        }
        _ => return None,
    })
}

/// Une carte en texte, pour le presse-papiers : un tableau en colonnes séparées par des tabulations, en-tête et pied
/// compris (il se colle dans Excel cellule par cellule) ; un agenda, son titre, puis un jour par paragraphe et un
/// événement par ligne ; un brouillon, son destinataire, son objet et son extrait ; un aperçu, son titre et son chemin.
/// `None` : une carte qui ne se lit pas.
pub fn en_texte(json: &str) -> Option<String> {
    // Dans une cellule, une tabulation ou un saut de ligne changerait de case ou de rangée.
    let cellule = |c: &str| c.replace(['\t', '\r', '\n'], " ");
    let rangee = |r: &[String]| r.iter().map(|c| cellule(c)).collect::<Vec<_>>().join("\t");
    Some(match lire_carte(json)? {
        Carte::Tableau(t) => {
            let entete = (!t.colonnes.is_empty()).then(|| rangee(&t.colonnes));
            entete.into_iter().chain(t.lignes.iter().chain(&t.pieds).map(|r| rangee(r))).collect::<Vec<_>>().join("\n")
        }
        Carte::Agenda(a) => {
            let mut paragraphes = vec![a.titre.clone()];
            for j in &a.jours {
                let mut lignes: Vec<String> = (!j.jour.is_empty()).then(|| j.jour.clone()).into_iter().collect();
                for e in &j.evenements {
                    let quand = if e.heure.is_empty() { String::new() } else { format!("{} · ", e.heure) };
                    let detail = if e.detail.is_empty() { String::new() } else { format!(" — {}", e.detail) };
                    lignes.push(format!("{quand}{}{detail}", e.titre));
                }
                paragraphes.push(lignes.join("\n"));
            }
            paragraphes.join("\n\n")
        }
        Carte::Brouillon(b) => format!("{}\nÀ : {}\nObjet : {}\n\n{}", b.titre, b.a, b.objet, b.extrait),
        Carte::Apercu(c) => format!("{} : {}", c.titre, c.chemin),
        Carte::Rappel { a, texte } => format!("Rappel, {} : {texte}", quand_du_rappel(&a)),
        Carte::Epingle { titre, retiree: false, .. } => format!("{titre} (épinglée au tableau de bord)"),
        Carte::Epingle { titre, retiree: true, .. } => format!("{titre} (retirée du tableau de bord)"),
    })
}

/// L'heure d'une carte « rappel », telle qu'on la dit (« 16 h », « demain à 9 h ») ; telle qu'écrite si elle ne se lit
/// pas.
fn quand_du_rappel(a: &str) -> String {
    crate::rappels::heure_ecrite(a)
        .map_or_else(|| a.to_string(), |a| crate::rappels::heure_dite(&a, &crate::journal::date()))
}

/// Une valeur d'une carte, en texte, quelle qu'elle soit (`txt` dans la page) : rien pour `null`, le JSON d'un objet.
fn texte(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(x) => x.to_string(),
    }
}

fn ou(t: String, defaut: &str) -> String {
    if t.is_empty() { defaut.to_string() } else { t }
}

/// Les jours d'un agenda : `jours`, à défaut `evenements` seul (un jour sans titre). Un jour peut n'être qu'une
/// chaîne (son titre), un événement aussi.
fn lire_jours(d: &serde_json::Map<String, Value>) -> Vec<Jour> {
    let jours: Vec<Value> = match (d.get("jours"), d.get("evenements")) {
        (Some(Value::Array(j)), _) => j.clone(),
        (_, Some(Value::Array(e))) => vec![serde_json::json!({ "evenements": e })],
        _ => Vec::new(),
    };
    jours
        .iter()
        .map(|j| {
            // Comme la page : un objet (un tableau en est un aussi pour elle) porte ses champs ; le reste est un titre.
            let (jour, evenements) = match j {
                Value::Object(o) => (texte(o.get("jour")), o.get("evenements")),
                Value::Array(_) => (String::new(), None),
                x => (texte(Some(x)), None),
            };
            let evenements = evenements.and_then(Value::as_array).map(|l| l.iter().map(lire_evenement).collect());
            Jour { jour, evenements: evenements.unwrap_or_default() }
        })
        .collect()
}

fn lire_evenement(e: &Value) -> Evenement {
    match e {
        Value::Object(o) => Evenement {
            heure: texte(o.get("heure")),
            titre: texte(o.get("titre")),
            detail: texte(o.get("detail")),
            sorte: o.get("sorte").and_then(Value::as_str).and_then(SorteEv::lire),
        },
        Value::Array(_) => Evenement { heure: String::new(), titre: String::new(), detail: String::new(), sorte: None },
        x => Evenement { heure: String::new(), titre: texte(Some(x)), detail: String::new(), sorte: None },
    }
}

fn lire_tableau(d: &serde_json::Map<String, Value>) -> Tableau {
    let rangee = |l: &Value| -> Vec<String> {
        match l {
            Value::Array(a) => a.iter().map(|x| texte(Some(x))).collect(),
            x => vec![texte(Some(x))],
        }
    };
    let liste = |cle: &str| d.get(cle).and_then(Value::as_array).cloned().unwrap_or_default();
    let colonnes: Vec<String> = liste("colonnes").iter().map(|x| texte(Some(x))).collect();
    let lignes: Vec<Vec<String>> = liste("lignes").iter().map(rangee).collect();
    // Le pied : une rangée, ou un tableau de rangées.
    let pied = liste("pied");
    let pieds: Vec<Vec<String>> = match pied.first() {
        None => Vec::new(),
        Some(Value::Array(_)) => pied.iter().map(rangee).collect(),
        Some(_) => vec![rangee(&Value::Array(pied.clone()))],
    };
    let n = lignes.iter().chain(&pieds).map(Vec::len).chain([colonnes.len()]).max().unwrap_or(0);
    let a_droite = colonnes_a_droite(&lignes, &pieds, n);
    Tableau { titre: ou(texte(d.get("titre")), "Tableau"), colonnes, lignes, pieds, n, a_droite }
}

/// Les colonnes de nombres : la moitié au moins de leurs cellules pleines ressemblent à un nombre (les lignes, ou le
/// pied s'il n'y a pas de lignes).
fn colonnes_a_droite(lignes: &[Vec<String>], pieds: &[Vec<String>], n: usize) -> Vec<bool> {
    let source = if lignes.is_empty() { pieds } else { lignes };
    (0..n)
        .map(|k| {
            let pleines: Vec<&str> =
                source.iter().filter_map(|l| l.get(k)).map(|v| v.trim()).filter(|v| !v.is_empty()).collect();
            let nombres = pleines.iter().filter(|v| ressemble_a_un_nombre(v)).count();
            nombres > 0 && nombres * 2 >= pleines.len()
        })
        .collect()
}

/// Un nombre ou un montant, comme le reconnaît la page (`NOMBRE`) : « 937,05 $ », « 1 234,56 », « -12 % »,
/// « (45,00 $) », « 8 h », « 2026 », « 12 km », « 3,5 m² », « 17 Ko »… Un signe, un dollar devant, des chiffres,
/// des espaces (insécables comprises), des points, des virgules et des apostrophes, puis une unité.
fn ressemble_a_un_nombre(v: &str) -> bool {
    // Les milliers, les millions et les milliards de dollars comme on les écrit au Québec (« 12 k$ », « 1,2 M$ »).
    const UNITES: [&str; 19] = [
        "$", "k$", "m$", "g$", "%", "h", "min", "j", "km", "kg", "m²", "m2", "pi²", "pi2", "pi", "po", "ko", "mo", "go",
    ];
    let blanc = |c: char| c.is_whitespace() || c == '\u{feff}';
    let s = v.trim_matches(blanc);
    let s = s.strip_prefix('(').unwrap_or(s);
    let s = s.strip_prefix(['−', '–', '+', '-']).unwrap_or(s).trim_start_matches(blanc);
    let s = s.strip_prefix('$').unwrap_or(s).trim_start_matches(blanc);
    let mut c = s.chars();
    if !c.next().is_some_and(|x| x.is_ascii_digit()) {
        return false;
    }
    let reste =
        c.as_str().trim_start_matches(|x: char| x.is_ascii_digit() || blanc(x) || matches!(x, '.' | ',' | '\''));
    let fin = |t: &str| {
        let t = t.trim_start_matches(blanc);
        t.strip_prefix(')').unwrap_or(t).is_empty()
    };
    let bas = reste.to_lowercase();
    fin(&bas) || UNITES.iter().any(|u| bas.strip_prefix(u).is_some_and(fin))
}

/// La sorte d'aperçu d'un nom de fichier, d'après son extension.
fn sorte_de_fichier(nom: &str) -> Option<SorteFichier> {
    let (_, ext) = nom.rsplit_once('.')?;
    if ext.is_empty() || !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(match ext.to_ascii_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" => SorteFichier::Image,
        "svg" => SorteFichier::Svg,
        "pdf" => SorteFichier::Pdf,
        "html" | "htm" => SorteFichier::Page,
        _ => return None,
    })
}

/// Le texte comme `white-space: pre-line` : les retours gardés, les blancs de chaque ligne resserrés.
fn en_lignes(texte: &str) -> String {
    texte
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|l| l.split([' ', '\t']).filter(|m| !m.is_empty()).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// « 1 ligne », « 12 lignes ».
fn nombre_de(n: u64, nom: &str) -> String {
    if n == 1 { format!("1 {nom}") } else { format!("{n} {nom}s") }
}

/// « 17 Ko », « 2,3 Mo » (comme la page) : la taille d'un aperçu, celle d'une pièce jointe.
pub(crate) fn taille_lisible(octets: u64) -> String {
    if octets < 1024 * 1024 {
        format!("{} Ko", ((octets as f64 / 1024.0).round() as u64).max(1))
    } else {
        format!("{:.1} Mo", octets as f64 / 1_048_576.0).replace('.', ",")
    }
}

// ---- Le jugement d'un chemin (le correctif n° 5) -------------------------------------------------------------------

/// Un chemin complet d'un disque du poste, normalisé : la lettre du disque, en capitale, et ses composants, sans « . »
/// ni « .. ».
#[derive(Debug, Clone, PartialEq)]
struct Chemin {
    disque: char,
    composants: Vec<String>,
}

impl Chemin {
    fn chemin(&self) -> PathBuf {
        PathBuf::from(format!("{}:\\{}", self.disque, self.composants.join("\\")))
    }

    /// Le chemin est sous `racine` (ou est elle) : composant par composant, la casse comptant comme pour Windows.
    fn sous(&self, racine: &Chemin) -> bool {
        self.disque == racine.disque
            && self.composants.len() >= racine.composants.len()
            && racine.composants.iter().zip(&self.composants).all(|(a, b)| meme_nom(a, b))
    }
}

/// Deux noms de fichier égaux pour Windows : sans compter la casse, celle de chaque lettre prise seule (« É » vaut
/// « é », mais « ß » ne vaut pas « SS » : la table de NTFS ne change jamais un caractère en deux).
fn meme_nom(a: &str, b: &str) -> bool {
    let majuscule = |c: char| {
        let mut m = c.to_uppercase();
        match (m.next(), m.next()) {
            (Some(x), None) => x,
            _ => c,
        }
    };
    a.chars().map(majuscule).eq(b.chars().map(majuscule))
}

/// Pourquoi un chemin ne s'ouvre pas, sur sa seule forme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refus {
    /// Un chemin réseau (« \\serveur\partage », « \\?\UNC\… ») : l'ouvrir, c'est s'authentifier auprès du serveur.
    Reseau,
    /// Pas le chemin complet d'un fichier d'un disque du poste : relatif, un périphérique, un nom réservé…
    Incomplet,
    /// Hors du dossier de travail et du dossier local.
    Dehors,
}

/// Le chemin qu'écrit une carte, normalisé sans toucher au disque : les « / » deviennent des « \ », le préfixe
/// « \\?\ » d'un chemin étendu tombe, « . » et « .. » se résolvent. Seul passe le chemin complet d'un disque du poste
/// (« C:\… ») ; un chemin réseau, un chemin de périphérique (« \\.\pipe\… »), un chemin relatif (« plan.svg »,
/// « \plan.svg », « C:plan.svg »), un nom réservé de Windows (« CON », « NUL.png »…) ou un caractère interdit
/// (« : » d'un flux de données, « * », « ? »…) sont refusés.
fn normaliser(brut: &str) -> Result<Chemin, Refus> {
    let s = brut.trim().replace('/', "\\");
    if s.is_empty() || s.chars().any(char::is_control) {
        return Err(Refus::Incomplet);
    }
    // Les préfixes des chemins étendus et de périphérique : seul tombe celui d'un disque du poste.
    let s = match ["\\\\?\\", "\\??\\", "\\\\.\\"].into_iter().find(|p| s.starts_with(p)) {
        Some(p) => {
            let reste = &s[p.len()..];
            let unc = reste.get(..3).is_some_and(|x| x.eq_ignore_ascii_case("UNC"))
                && matches!(reste.as_bytes().get(3), None | Some(b'\\'));
            if unc {
                return Err(Refus::Reseau);
            }
            reste.to_string()
        }
        None if s.starts_with("\\\\") => return Err(Refus::Reseau),
        None => s,
    };
    let mut c = s.chars();
    let (Some(disque), Some(':'), Some('\\')) = (c.next(), c.next(), c.next()) else {
        return Err(Refus::Incomplet);
    };
    if !disque.is_ascii_alphabetic() {
        return Err(Refus::Incomplet);
    }
    let mut composants: Vec<String> = Vec::new();
    for x in c.as_str().split('\\') {
        match x {
            "" | "." => {}
            ".." => {
                composants.pop();
            }
            _ if x.contains(['<', '>', ':', '"', '|', '?', '*']) || nom_reserve(x) => return Err(Refus::Incomplet),
            _ => composants.push(x.to_string()),
        }
    }
    Ok(Chemin { disque: disque.to_ascii_uppercase(), composants })
}

/// Les noms que Windows réserve à ses périphériques, avec ou sans extension : lire « CON.png » attendrait la console.
fn nom_reserve(composant: &str) -> bool {
    let base = composant.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    let numero = |prefixe: &str| {
        base.strip_prefix(prefixe).is_some_and(|n| {
            let mut c = n.chars();
            matches!((c.next(), c.next()), (Some('0'..='9' | '¹' | '²' | '³'), None))
        })
    };
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") || numero("COM") || numero("LPT")
}

/// Le premier jugement, sur la forme : un chemin normalisé, sous l'une des racines.
fn juger(brut: &str, racines: &[Chemin]) -> Result<Chemin, Refus> {
    let c = normaliser(brut)?;
    if racines.iter().any(|r| c.sous(r)) { Ok(c) } else { Err(Refus::Dehors) }
}

/// Ce que désigne vraiment un chemin, sur le disque (Windows résout les jonctions, les liens, les noms courts), sous la
/// même forme ; un fichier qui n'existe pas n'a pas de forme réelle.
fn resoudre(chemin: &Path) -> Result<Chemin, Refus> {
    let reel = std::fs::canonicalize(chemin).map_err(|_| Refus::Incomplet)?;
    // Windows le rend en chemin étendu : « \\?\C:\… », ou « \\?\UNC\… » s'il mène au réseau.
    normaliser(&reel.to_string_lossy())
}

// ---- Le fil de lecture des aperçus ---------------------------------------------------------------------------------

/// La taille et la date d'un fichier : s'il les garde, il n'a pas changé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Empreinte {
    taille: u64,
    date: Option<SystemTime>,
}

/// Ce que la fenêtre demande au fil : lire un fichier dont la forme du chemin est déjà jugée.
struct Demande {
    /// Le chemin tel que l'écrit la carte : la clé de l'aperçu.
    cle: String,
    /// Le chemin normalisé : le seul qu'on ouvre.
    chemin: PathBuf,
    sorte: SorteFichier,
    /// La version déjà montrée : si le fichier la garde, rien ne se relit.
    connu: Option<Empreinte>,
    /// Le grand côté d'une texture.
    limite: usize,
    ctx: egui::Context,
}

/// Ce que le fil rend.
enum Lu {
    Image { image: egui::ColorImage, intrinseque: Vec2, pages: u32, empreinte: Empreinte },
    Page { titre: String, extrait: String, empreinte: Empreinte },
    Inchange,
    Echec { probleme: Probleme, existe: bool, empreinte: Option<Empreinte> },
}

/// Ce qu'un aperçu ou un dessin dit quand il ne s'affiche pas.
#[derive(Debug, Clone, PartialEq)]
enum Probleme {
    Refuse(Refus),
    Introuvable,
    TropGros(u64),
    Illisible,
    PdfIllisible,
    SorteInconnue,
    /// Un dessin dont le SVG ne se lit pas, ou trop gros (sa taille, en octets).
    DessinIllisible,
    DessinTropGros(usize),
}

impl Probleme {
    fn message(&self) -> String {
        let debut = "L'aperçu ne s'affiche pas : ";
        match self {
            Probleme::DessinIllisible => "Ce dessin ne s'affiche pas : son SVG ne se lit pas.".to_string(),
            Probleme::DessinTropGros(n) => format!(
                "Ce dessin ne s'affiche pas : son SVG est trop gros ({}, quand un dessin en prend 512 Ko au plus).",
                taille_lisible(*n as u64)
            ),
            Probleme::Refuse(Refus::Reseau) => format!("{debut}Eve n'ouvre pas de fichier sur le réseau."),
            Probleme::Refuse(Refus::Dehors) => {
                format!("{debut}Eve n'ouvre que les fichiers de son dossier de travail.")
            }
            Probleme::Refuse(Refus::Incomplet) => {
                format!("{debut}ce n'est pas le chemin complet d'un fichier du poste.")
            }
            Probleme::Introuvable => format!("{debut}le fichier est introuvable."),
            Probleme::TropGros(n) => {
                format!(
                    "{debut}le fichier est trop gros ({}, quand un aperçu en prend 25 au plus).",
                    taille_lisible(*n)
                )
            }
            Probleme::Illisible => format!("{debut}le fichier ne se lit pas."),
            Probleme::PdfIllisible => format!("{debut}le PDF ne s'ouvre pas (peut-être protégé par un mot de passe)."),
            Probleme::SorteInconnue => "Ce genre de fichier ne s'affiche pas en aperçu.".to_string(),
        }
    }

    fn icone(&self) -> (char, Color32) {
        match self {
            Probleme::Refuse(_) => (pictos::BOUCLIER, couleurs::AMBRE),
            Probleme::SorteInconnue => (pictos::INFO, couleurs::ENCRE_3),
            Probleme::TropGros(_) | Probleme::DessinTropGros(_) => (icones::ALERTE, couleurs::AMBRE),
            _ => (icones::ALERTE, couleurs::ENCRE_3),
        }
    }
}

/// Ce que la fenêtre demande au fil de dessiner : un dessin d'Eve, à `largeur` pixels.
struct DemandeDeDessin {
    cle: u64,
    svg: Arc<str>,
    largeur: u32,
    /// Le grand côté d'une texture.
    limite: usize,
    ctx: egui::Context,
}

/// Ce que le fil rend d'un dessin : l'image et sa taille d'origine (son viewBox) ; `None` s'il ne se lit pas.
type DessinRendu = Option<(egui::ColorImage, Vec2)>;

/// Un travail du fil : lire l'aperçu d'un fichier, ou dessiner un SVG d'Eve.
enum Travail {
    Apercu(Demande),
    Dessin(DemandeDeDessin),
}

/// Le fil qui lit les fichiers des aperçus et dessine les dessins : il naît au premier travail et dépose ce qu'il a fait
/// dans `rendus` et `dessins`.
struct Lecture {
    /// Les dossiers permis, tels quels : le fil les résout sur le disque.
    racines: Arc<Vec<PathBuf>>,
    envoi: Option<mpsc::Sender<Travail>>,
    rendus: Arc<Mutex<Vec<(String, Lu)>>>,
    dessins: Arc<Mutex<Vec<(u64, DessinRendu)>>>,
}

impl Lecture {
    /// Confie une lecture au fil ; `false` s'il ne la prend pas (il n'a pas pu naître, ou s'est arrêté).
    fn demander(&mut self, d: Demande) -> bool {
        self.confier(Travail::Apercu(d))
    }

    /// Confie un dessin au fil ; `false` s'il ne le prend pas.
    fn dessiner(&mut self, d: DemandeDeDessin) -> bool {
        self.confier(Travail::Dessin(d))
    }

    fn confier(&mut self, t: Travail) -> bool {
        if self.envoi.is_none() {
            let (envoi, recu) = mpsc::channel::<Travail>();
            let (racines, rendus, dessins) = (self.racines.clone(), self.rendus.clone(), self.dessins.clone());
            let ne = thread::Builder::new()
                .name("eve-apercus".into())
                .spawn(move || lire_sans_fin(&recu, &racines, &rendus, &dessins));
            if ne.is_err() {
                return false;
            }
            self.envoi = Some(envoi);
        }
        self.envoi.as_ref().is_some_and(|e| e.send(t).is_ok())
    }
}

/// Le fil : un travail après l'autre, chacun rendu à la fenêtre, qui se redessine. Un dessin qui panique (un SVG piégé)
/// devient un dessin illisible, sans emporter le fil.
fn lire_sans_fin(
    recu: &mpsc::Receiver<Travail>,
    racines: &[PathBuf],
    rendus: &Mutex<Vec<(String, Lu)>>,
    dessins: &Mutex<Vec<(u64, DessinRendu)>>,
) {
    #[cfg(windows)]
    systeme::initialiser();
    for t in recu {
        match t {
            Travail::Apercu(d) => {
                let lu = lire(&d, racines);
                rendus.lock().unwrap_or_else(|e| e.into_inner()).push((d.cle.clone(), lu));
                d.ctx.request_repaint();
            }
            Travail::Dessin(d) => {
                let rendu = panic::catch_unwind(AssertUnwindSafe(|| dessin_en_pixels(&d.svg, d.largeur, d.limite)));
                dessins.lock().unwrap_or_else(|e| e.into_inner()).push((d.cle, rendu.ok().and_then(Result::ok)));
                d.ctx.request_repaint();
            }
        }
    }
}

/// Une lecture : le second jugement du chemin, sur ce qu'il désigne vraiment ; la taille ; puis le fichier, lu et
/// dessiné. Un dessin qui panique (un SVG piégé) devient un fichier illisible, sans emporter le fil.
fn lire(d: &Demande, racines: &[PathBuf]) -> Lu {
    let echec = |probleme, existe, empreinte| Lu::Echec { probleme, existe, empreinte };
    // Le chemin résolu (jonctions et liens compris) doit rester local et sous une racine elle aussi résolue.
    let reel = match resoudre(&d.chemin) {
        Ok(c) => c,
        Err(Refus::Reseau) => return echec(Probleme::Refuse(Refus::Reseau), false, None),
        Err(_) => return echec(Probleme::Introuvable, false, None),
    };
    let racines: Vec<Chemin> = racines.iter().filter_map(|r| resoudre(r).ok()).collect();
    if !racines.iter().any(|r| reel.sous(r)) {
        return echec(Probleme::Refuse(Refus::Dehors), false, None);
    }
    let chemin = reel.chemin();
    let Some(meta) = std::fs::metadata(&chemin).ok().filter(|m| m.is_file()) else {
        return echec(Probleme::Introuvable, false, None);
    };
    let empreinte = Empreinte { taille: meta.len(), date: meta.modified().ok() };
    if d.connu == Some(empreinte) {
        return Lu::Inchange;
    }
    if meta.len() > APERCU_MAX {
        return echec(Probleme::TropGros(meta.len()), true, Some(empreinte));
    }
    let mut octets = Vec::new();
    let lu = std::fs::File::open(&chemin).and_then(|f| f.take(APERCU_MAX + 1).read_to_end(&mut octets));
    if lu.is_err() || octets.len() as u64 > APERCU_MAX {
        return echec(Probleme::Illisible, true, None);
    }
    match panic::catch_unwind(AssertUnwindSafe(|| rendre(d.sorte, &octets, d.limite))) {
        Ok(Ok(Rendu::Image { image, intrinseque, pages })) => Lu::Image { image, intrinseque, pages, empreinte },
        Ok(Ok(Rendu::Page { titre, extrait })) => Lu::Page { titre, extrait, empreinte },
        Ok(Err(p)) => echec(p, true, Some(empreinte)),
        Err(_) => echec(Probleme::Illisible, true, Some(empreinte)),
    }
}

/// Un fichier dessiné : une image (et sa taille d'origine, en points ; le nombre de pages d'un PDF), ou ce qu'une page
/// HTML dit d'elle.
enum Rendu {
    Image { image: egui::ColorImage, intrinseque: Vec2, pages: u32 },
    Page { titre: String, extrait: String },
}

fn rendre(sorte: SorteFichier, octets: &[u8], limite: usize) -> Result<Rendu, Probleme> {
    match sorte {
        SorteFichier::Image => systeme::image(octets, limite).map(|(image, [l, h])| Rendu::Image {
            image,
            intrinseque: vec2(l as f32, h as f32),
            pages: 1,
        }),
        SorteFichier::Pdf => systeme::pdf(octets, limite),
        SorteFichier::Svg => svg(octets, limite),
        SorteFichier::Page => {
            let (titre, extrait) = vignette_html(&String::from_utf8_lossy(octets));
            Ok(Rendu::Page { titre, extrait })
        }
    }
}

/// Les polices du poste, pour le texte des SVG : chargées une fois pour toutes, au premier SVG (une fraction de
/// seconde), dans le fil qui le dessine.
fn polices() -> Arc<fontdb::Database> {
    static POLICES: OnceLock<Arc<fontdb::Database>> = OnceLock::new();
    POLICES
        .get_or_init(|| {
            let mut base = fontdb::Database::new();
            base.load_system_fonts();
            Arc::new(base)
        })
        .clone()
}

/// Les options de resvg : les polices du poste, et `police` pour un texte qui n'en nomme pas (sinon celle de resvg) ;
/// aucun autre fichier — par défaut, resvg lit l'image d'un « <image href="…"> » n'importe où sur le poste, ou sur le
/// réseau (Windows s'y authentifierait). Seules restent les images incrustées dans le SVG.
fn options_svg(police: Option<&str>) -> resvg::usvg::Options<'static> {
    use resvg::usvg;
    let mut options = usvg::Options {
        fontdb: polices(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    };
    if let Some(p) = police {
        options.font_family = p.to_string();
    }
    options
}

/// Un SVG dessiné par resvg, à deux fois sa taille (un écran à 200 %) sans dépasser la texture.
fn svg(octets: &[u8], limite: usize) -> Result<Rendu, Probleme> {
    use resvg::{tiny_skia, usvg};
    let arbre = usvg::Tree::from_data(octets, &options_svg(None)).map_err(|_| Probleme::Illisible)?;
    let (l, h) = (arbre.size().width(), arbre.size().height());
    let k = 2.0f32.min(limite as f32 / l.max(h));
    let cote = |x: f32| ((x * k).round() as u32).clamp(1, limite as u32);
    let (lp, hp) = (cote(l), cote(h));
    let mut toile = tiny_skia::Pixmap::new(lp, hp).ok_or(Probleme::Illisible)?;
    resvg::render(&arbre, tiny_skia::Transform::from_scale(lp as f32 / l, hp as f32 / h), &mut toile.as_mut());
    let image = egui::ColorImage::from_rgba_premultiplied([lp as usize, hp as usize], toile.data());
    Ok(Rendu::Image { image, intrinseque: vec2(l, h), pages: 1 })
}

/// Le titre (« <title> ») et le début du texte visible d'une page HTML, pour sa vignette : sans son en-tête, ses
/// scripts, ses styles, ses dessins ni ses commentaires ; les entités courantes décodées, les blancs resserrés.
fn vignette_html(html: &str) -> (String, String) {
    // Les mêmes positions en minuscules (l'ASCII seul change : les octets restent à leur place).
    let bas = html.to_ascii_lowercase();
    let titre = bas
        .find("<title")
        .and_then(|i| bas[i..].find('>').map(|j| i + j + 1))
        .and_then(|debut| bas[debut..].find("</title").map(|fin| &html[debut..debut + fin]))
        .map(nettoyer)
        .unwrap_or_default();
    let mut texte = String::new();
    let mut i = 0;
    while let Some(k) = bas[i..].find('<') {
        texte.push_str(&html[i..i + k]);
        let j = i + k;
        let apres = |motif: &str, depuis: usize| bas[depuis..].find(motif).map_or(html.len(), |p| depuis + p);
        let suite = if bas[j..].starts_with("<!--") {
            apres("-->", j) + 3
        } else if let Some(nom) = ["head", "script", "style", "noscript", "template", "svg", "title"]
            .iter()
            .find(|n| balise(&bas[j + 1..], n))
        {
            let fin = apres(&format!("</{nom}"), j);
            apres(">", fin) + 1
        } else {
            apres(">", j) + 1
        };
        i = suite.min(html.len());
        texte.push(' ');
    }
    texte.push_str(&html[i..]);
    let extrait: String = nettoyer(&texte).chars().take(400).collect();
    (titre, extrait)
}

/// `s` commence par la balise `nom` (« script », mais pas « scripts »).
fn balise(s: &str, nom: &str) -> bool {
    s.strip_prefix(nom).is_some_and(|r| r.chars().next().is_none_or(|c| c == '>' || c == '/' || c.is_whitespace()))
}

/// Un texte de page : ses entités décodées, sans caractère de contrôle, ses blancs resserrés.
fn nettoyer(t: &str) -> String {
    let mut s = String::new();
    let mut reste = t;
    while let Some(i) = reste.find('&') {
        s.push_str(&reste[..i]);
        let apres = &reste[i + 1..];
        match apres.find(';').filter(|f| *f <= 10).and_then(|f| entite(&apres[..f]).map(|c| (f, c))) {
            Some((f, c)) => {
                s.push_str(&c);
                reste = &apres[f + 1..];
            }
            None => {
                s.push('&');
                reste = apres;
            }
        }
    }
    s.push_str(reste);
    s.split(|c: char| c.is_whitespace() || c.is_control()).filter(|m| !m.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Une entité HTML : numérique, ou l'une des plus courantes en français.
fn entite(nom: &str) -> Option<String> {
    if let Some(n) = nom.strip_prefix('#') {
        let code = match n.strip_prefix(['x', 'X']) {
            Some(h) => u32::from_str_radix(h, 16).ok()?,
            None => n.parse().ok()?,
        };
        return char::from_u32(code).map(String::from);
    }
    let c = match nom {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        "nbsp" => " ",
        "eacute" => "é",
        "Eacute" => "É",
        "egrave" => "è",
        "ecirc" => "ê",
        "euml" => "ë",
        "agrave" => "à",
        "acirc" => "â",
        "ccedil" => "ç",
        "icirc" => "î",
        "iuml" => "ï",
        "ocirc" => "ô",
        "ugrave" => "ù",
        "ucirc" => "û",
        "laquo" => "«",
        "raquo" => "»",
        "lsquo" => "‘",
        "rsquo" => "’",
        "ldquo" => "“",
        "rdquo" => "”",
        "hellip" => "…",
        "ndash" => "–",
        "mdash" => "—",
        "middot" => "·",
        "deg" => "°",
        "euro" => "€",
        "copy" => "©",
        _ => return None,
    };
    Some(c.to_string())
}

/// Une image du disque (PNG, JPEG, GIF, WebP), décodée par Windows dans son bon sens, son grand côté dans `limite` :
/// le portrait d'Eve.
pub(crate) fn image_du_disque(chemin: &Path, limite: usize) -> Result<egui::ColorImage, String> {
    let octets = std::fs::read(chemin).map_err(|e| e.to_string())?;
    systeme::image(&octets, limite).map(|(image, _)| image).map_err(|p| p.message())
}

/// Ce que Windows sait lire lui-même (WinRT) : les images, dans leur bon sens, et la première page d'un PDF.
#[cfg(windows)]
mod systeme {
    use std::path::PathBuf;

    use eframe::egui::{ColorImage, vec2};
    use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
    use windows::Graphics::Imaging::{
        BitmapAlphaMode, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
        ColorManagementMode, ExifOrientationMode,
    };
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

    use super::{Probleme, Rendu};

    /// La boîte où se rend la première page d'un PDF : la plus large des cartes, ou sa hauteur, sur un écran à 200 %
    /// et plus.
    const PAGE_BOITE: [f32; 2] = [1800.0, 1000.0];

    pub fn initialiser() {
        // SAFETY : WinRT pour ce fil (un appartement multithread), avant tout objet WinRT ; un second appel est sans
        // effet.
        let _ = unsafe {
            windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED)
        };
    }

    fn w<T>(r: windows::core::Result<T>) -> Result<T, Probleme> {
        r.map_err(|_| Probleme::Illisible)
    }

    /// Des octets en mémoire, en flux WinRT.
    fn flux(octets: &[u8]) -> windows::core::Result<InMemoryRandomAccessStream> {
        let flux = InMemoryRandomAccessStream::new()?;
        let ecrivain = DataWriter::CreateDataWriter(&flux)?;
        ecrivain.WriteBytes(octets)?;
        ecrivain.StoreAsync()?.join()?;
        // Le flux reste à qui le lit quand l'écrivain s'en va.
        ecrivain.DetachStream()?;
        flux.Seek(0)?;
        Ok(flux)
    }

    /// Une image (PNG, JPEG, GIF — sa première image —, WebP) en pixels : dans son bon sens (l'orientation EXIF d'une
    /// photo), ses couleurs ramenées au sRGB, réduite pour que son grand côté tienne dans `limite` ; et sa taille
    /// d'origine, dans ce sens.
    pub fn image(octets: &[u8], limite: usize) -> Result<(ColorImage, [u32; 2]), Probleme> {
        pixels(&w(flux(octets))?, limite)
    }

    fn pixels(flux: &InMemoryRandomAccessStream, limite: usize) -> Result<(ColorImage, [u32; 2]), Probleme> {
        let d = w(BitmapDecoder::CreateAsync(flux).and_then(|o| o.join()))?;
        let (l, h) = (w(d.PixelWidth())?, w(d.PixelHeight())?);
        let (lo, ho) = (w(d.OrientedPixelWidth())?, w(d.OrientedPixelHeight())?);
        let k = (limite as f64 / f64::from(l.max(h).max(1))).min(1.0);
        let cote = |x: u32| ((f64::from(x) * k).round() as u32).max(1);
        let (ls, hs) = (cote(l), cote(h));
        let t = w(BitmapTransform::new())?;
        w(t.SetScaledWidth(ls))?;
        w(t.SetScaledHeight(hs))?;
        w(t.SetInterpolationMode(BitmapInterpolationMode::Fant))?;
        let donnees = w(d
            .GetPixelDataTransformedAsync(
                BitmapPixelFormat::Rgba8,
                BitmapAlphaMode::Premultiplied,
                &t,
                ExifOrientationMode::RespectExifOrientation,
                ColorManagementMode::ColorManageToSRgb,
            )
            .and_then(|o| o.join())
            .and_then(|p| p.DetachPixelData()))?;
        // La réduction se fait avant l'orientation : une photo tournée d'un quart de tour échange ses côtés.
        let (lr, hr) = if lo == h && ho == l && l != h { (hs, ls) } else { (ls, hs) };
        if donnees.len() != lr as usize * hr as usize * 4 {
            return Err(Probleme::Illisible);
        }
        Ok((ColorImage::from_rgba_premultiplied([lr as usize, hr as usize], &donnees), [lo, ho]))
    }

    /// La première page d'un PDF, rendue par Windows sur fond blanc ; sa taille en points et le nombre de pages.
    pub fn pdf(octets: &[u8], limite: usize) -> Result<Rendu, Probleme> {
        let source = w(flux(octets))?;
        let document =
            PdfDocument::LoadFromStreamAsync(&source).and_then(|o| o.join()).map_err(|_| Probleme::PdfIllisible)?;
        let pages = w(document.PageCount())?;
        if pages == 0 {
            return Err(Probleme::PdfIllisible);
        }
        let page = w(document.GetPage(0))?;
        let taille = w(page.Size())?;
        let (l, h) = (taille.Width.max(1.0), taille.Height.max(1.0));
        let plafond = limite as f32;
        let k = (PAGE_BOITE[0].min(plafond) / l).min(PAGE_BOITE[1].min(plafond) / h);
        let cote = |x: f32| ((x * k).round() as u32).clamp(1, limite as u32);
        let options = w(PdfPageRenderOptions::new())?;
        w(options.SetDestinationWidth(cote(l)))?;
        w(options.SetDestinationHeight(cote(h)))?;
        let sortie = w(InMemoryRandomAccessStream::new())?;
        w(page.RenderWithOptionsToStreamAsync(&sortie, &options).and_then(|a| a.join()))?;
        let _ = page.Close();
        w(sortie.Seek(0))?;
        let (image, _) = pixels(&sortie, limite)?;
        Ok(Rendu::Image { image, intrinseque: vec2(l, h), pages })
    }

    /// La boîte « Enregistrer sous » de Windows, pour un dessin : `nom` proposé, en PNG d'office, ou en SVG ; modale
    /// au-dessus de la fenêtre `proprietaire` (0 : d'aucune). Elle tient dans le fil qui l'appelle, COM en appartement
    /// (STA, ce qu'elle veut), comme celle des pièces jointes. Rend le fichier choisi, avec son extension ; rien si on
    /// l'annule ; ce qui l'empêche de s'ouvrir.
    pub fn enregistrer_sous(proprietaire: isize, nom: &str) -> Result<Option<PathBuf>, String> {
        use std::os::windows::ffi::OsStringExt;

        use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
        use windows::Win32::System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize,
        };
        use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
        use windows::Win32::UI::Shell::{
            FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT, FileSaveDialog, IFileSaveDialog,
            SIGDN_FILESYSPATH,
        };
        use windows::core::{PCWSTR, w};

        let nom: Vec<u16> = nom.encode_utf16().chain([0]).collect();
        // SAFETY : COM pour ce fil, défait à la fin si l'initialisation a réussi.
        let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        let choisi = || -> windows::core::Result<(PathBuf, u32)> {
            // SAFETY : la boîte de dialogue et son résultat ne vivent que dans cette fermeture, avant que COM se défasse ;
            // le nom proposé vit jusqu'à la fin de la fonction ; celui que rend Windows se libère par CoTaskMemFree, une
            // fois lu.
            unsafe {
                let dialogue: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
                dialogue
                    .SetOptions(dialogue.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR | FOS_OVERWRITEPROMPT)?;
                let types = [
                    COMDLG_FILTERSPEC { pszName: w!("Image PNG"), pszSpec: w!("*.png") },
                    COMDLG_FILTERSPEC { pszName: w!("Dessin SVG"), pszSpec: w!("*.svg") },
                ];
                dialogue.SetFileTypes(&types)?;
                dialogue.SetFileTypeIndex(1)?;
                dialogue.SetDefaultExtension(w!("png"))?;
                dialogue.SetFileName(PCWSTR(nom.as_ptr()))?;
                dialogue.Show((proprietaire != 0).then_some(HWND(proprietaire as *mut core::ffi::c_void)))?;
                let choisi = dialogue.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
                let chemin = PathBuf::from(std::ffi::OsString::from_wide(choisi.as_wide()));
                CoTaskMemFree(Some(choisi.0 as *const core::ffi::c_void));
                Ok((chemin, dialogue.GetFileTypeIndex()?))
            }
        };
        let choisi = choisi();
        if com.is_ok() {
            // SAFETY : l'initialisation de ce fil a réussi ; ce qui était de COM s'en est allé avec la fermeture.
            unsafe { CoUninitialize() };
        }
        match choisi {
            Ok((chemin, type_choisi)) => Ok(Some(super::avec_extension(chemin, type_choisi))),
            // Annulée, elle rend une erreur : rien n'est choisi.
            Err(e) if e.code() == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) => Ok(None),
            Err(e) => Err(e.message()),
        }
    }
}

/// Ailleurs que sous Windows, ni les images ni le PDF ne se lisent ici (le SVG et la page HTML, si), et un dessin ne
/// s'enregistre pas.
#[cfg(not(windows))]
mod systeme {
    use std::path::PathBuf;

    use eframe::egui::ColorImage;

    use super::{Probleme, Rendu};

    pub fn image(_: &[u8], _: usize) -> Result<(ColorImage, [u32; 2]), Probleme> {
        Err(Probleme::Illisible)
    }

    pub fn pdf(_: &[u8], _: usize) -> Result<Rendu, Probleme> {
        Err(Probleme::PdfIllisible)
    }

    pub fn enregistrer_sous(_: isize, _: &str) -> Result<Option<PathBuf>, String> {
        Ok(None)
    }
}

// ---- L'aperçu à l'écran --------------------------------------------------------------------------------------------

/// Un aperçu, d'une image à l'autre : ce qu'on montre, la version du fichier, la lecture en cours.
struct Apercu {
    etat: Etat,
    /// La version du fichier montrée (sa taille et sa date) ; `None` tant qu'il n'a pas été lu.
    empreinte: Option<Empreinte>,
    /// Une lecture est partie et n'est pas revenue.
    en_lecture: bool,
    /// Le moment de la dernière lecture demandée, et le dernier où la carte était à l'écran.
    demande: f64,
    vu: f64,
}

enum Etat {
    Attente,
    Image { texture: egui::TextureHandle, intrinseque: Vec2, pages: u32, taille: u64 },
    Page { titre: String, extrait: String, taille: u64 },
    Echec { probleme: Probleme, existe: bool },
}

impl Apercu {
    fn nouveau(temps: f64) -> Apercu {
        Apercu { etat: Etat::Attente, empreinte: None, en_lecture: false, demande: f64::NEG_INFINITY, vu: temps }
    }

    fn recevoir(&mut self, ctx: &egui::Context, cle: &str, lu: Lu) {
        self.en_lecture = false;
        match lu {
            Lu::Inchange => {}
            Lu::Image { image, intrinseque, pages, empreinte } => {
                // Des mipmaps : une image réduite dans la carte reste nette, sans scintiller.
                let options = egui::TextureOptions {
                    mipmap_mode: Some(egui::TextureFilter::Linear),
                    ..egui::TextureOptions::LINEAR
                };
                let texture = ctx.load_texture(format!("apercu:{cle}"), image, options);
                self.etat = Etat::Image { texture, intrinseque, pages, taille: empreinte.taille };
                self.empreinte = Some(empreinte);
            }
            Lu::Page { titre, extrait, empreinte } => {
                self.etat = Etat::Page { titre, extrait, taille: empreinte.taille };
                self.empreinte = Some(empreinte);
            }
            Lu::Echec { probleme, existe, empreinte } => {
                self.etat = Etat::Echec { probleme, existe };
                self.empreinte = empreinte;
            }
        }
    }

    /// Le fichier existe, et la carte peut l'ouvrir en grand.
    fn ouvrable(&self) -> bool {
        matches!(self.etat, Etat::Image { .. } | Etat::Page { .. } | Etat::Echec { existe: true, .. })
    }

    /// Ce que dit la tête de la carte, à droite : la sorte du fichier et sa taille, ou ses pages.
    fn precision(&self, sorte: SorteFichier, nom: &str) -> String {
        let ext = nom.rsplit_once('.').map_or(String::new(), |(_, e)| e.to_uppercase());
        match &self.etat {
            Etat::Image { pages, .. } if sorte == SorteFichier::Pdf => {
                format!("PDF · {}", nombre_de(u64::from(*pages), "page"))
            }
            Etat::Image { taille, .. } if sorte == SorteFichier::Svg => format!("SVG · {}", taille_lisible(*taille)),
            Etat::Image { intrinseque, .. } => {
                format!("{ext} · {} × {}", intrinseque.x.round() as u32, intrinseque.y.round() as u32)
            }
            Etat::Page { taille, .. } => format!("HTML · {}", taille_lisible(*taille)),
            _ => String::new(),
        }
    }
}

/// La carte aperçu (AP1 à AP7) : le chemin jugé sur sa forme, la lecture confiée au fil (puis revérifiée tant que la
/// carte est à l'écran), et ce qu'elle a donné ; « Ouvrir en grand » quand le fichier est là.
fn apercu(
    ui: &mut egui::Ui,
    c: &CarteApercu,
    apercus: &mut HashMap<String, Apercu>,
    racines: &[Chemin],
    lecture: &mut Lecture,
    temps: f64,
    id: egui::Id,
) -> Option<Geste> {
    let picto = match c.sorte {
        Some(SorteFichier::Pdf) => pictos::PDF,
        Some(SorteFichier::Page) => icones::PAGE_WEB,
        _ => pictos::IMAGE,
    };
    let juge = match (c.sorte, juger(&c.chemin, racines)) {
        (None, _) => Err(Probleme::SorteInconnue),
        (_, Err(r)) => Err(Probleme::Refuse(r)),
        (Some(s), Ok(ch)) => Ok((s, ch)),
    };
    let (sorte, chemin) = match juge {
        Ok(x) => x,
        Err(p) => {
            cadre(ui, picto, &c.titre, "", None, |ui| avis(ui, &p));
            return None;
        }
    };
    let a = apercus.entry(c.chemin.clone()).or_insert_with(|| Apercu::nouveau(temps));
    // À l'écran, ou presque : la carte se lit (une conversation reprise ne lit pas d'un coup ce qu'on ne voit pas).
    let place = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 300.0));
    let proche = ui.is_rect_visible(place.expand(200.0));
    if proche {
        a.vu = temps;
        if !a.en_lecture && (matches!(a.etat, Etat::Attente) || temps - a.demande >= REVERIFIER) {
            let limite = ui.input(|i| i.max_texture_side).min(TEXTURE_MAX);
            let d = Demande {
                cle: c.chemin.clone(),
                chemin: chemin.chemin(),
                sorte,
                connu: a.empreinte,
                limite,
                ctx: ui.ctx().clone(),
            };
            a.demande = temps;
            a.en_lecture = lecture.demander(d);
            if !a.en_lecture {
                a.etat = Etat::Echec { probleme: Probleme::Illisible, existe: false };
            }
        }
    }
    let precision = a.precision(sorte, &c.nom);
    let ouvrable = a.ouvrable();
    let mut ouvrir = false;
    cadre(ui, picto, &c.titre, &precision, None, |ui| {
        ouvrir = match &a.etat {
            Etat::Attente => {
                attente(ui, "Lecture de l'aperçu…");
                false
            }
            Etat::Image { texture, intrinseque, .. } => {
                let taille = ajuster(*intrinseque, vec2(ui.available_width(), haut_max(ui)));
                image(ui, texture, taille, sorte == SorteFichier::Pdf, ouvrable, id)
            }
            Etat::Page { titre, extrait, .. } => vignette(ui, titre, extrait, &c.nom, ouvrable),
            Etat::Echec { probleme, .. } => {
                avis(ui, probleme);
                false
            }
        };
        if ouvrable {
            ui.add_space(10.0);
            ouvrir |= bouton_lien(ui, pictos::OUVRIR, "Ouvrir en grand").clicked();
        }
    });
    // Le chemin normalisé, celui qu'on a jugé : le chef l'ouvre tel quel.
    ouvrir.then(|| Geste { action: "ouvrir_fichier", valeur: Value::String(chemin.chemin().display().to_string()) })
}

/// Une image dans sa carte (AP1, AP2 ; AP3 pour la page d'un PDF ; un dessin), à `taille`, centrée, sur du blanc (un
/// dessin transparent se lit) ; une page de PDF (`feuille`) a l'ombre d'une feuille. Rend `true` si on clique dessus.
fn image(
    ui: &mut egui::Ui,
    texture: &egui::TextureHandle,
    taille: Vec2,
    feuille: bool,
    clic: bool,
    id: egui::Id,
) -> bool {
    let dispo = ui.available_width();
    let (zone, _) = ui.allocate_exact_size(vec2(dispo, taille.y), Sense::hover());
    let r = Rect::from_center_size(zone.center(), taille);
    // Seule l'image s'ouvre au clic, pas les marges d'une page plus étroite que la carte.
    let rep = ui.interact(r, id.with("image"), if clic { Sense::click() } else { Sense::hover() });
    let rayon = if feuille { 3.0 } else { 6.0 };
    let p = ui.painter();
    if feuille {
        let ombre = egui::Shadow { offset: [0, 4], blur: 16, spread: 0, color: teintes(ui).ombre };
        p.add(ombre.as_shape(r, rayon));
    }
    p.rect_filled(r, rayon, Color32::WHITE);
    egui::Image::from_texture((texture.id(), taille)).corner_radius(rayon).paint_at(ui, r);
    p.rect_stroke(r, rayon, Stroke::new(1.0, Color32::from_black_alpha(46)), StrokeKind::Inside);
    clic && rep.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Ouvrir en grand").clicked()
}

/// La taille d'une image qui tient dans `boite` sans s'agrandir, ses proportions gardées.
fn ajuster(taille: Vec2, boite: Vec2) -> Vec2 {
    if taille.x <= 0.0 || taille.y <= 0.0 {
        return vec2(boite.x.min(120.0), boite.y.min(120.0));
    }
    let k = (boite.x / taille.x).min(boite.y / taille.y).min(1.0);
    (taille * k).max(Vec2::splat(1.0))
}

/// La taille d'un dessin dans `boite` : toute sa largeur (un dessin se redessine net à toute taille), ses proportions
/// gardées, sans dépasser sa hauteur.
fn remplir(taille: Vec2, boite: Vec2) -> Vec2 {
    if taille.x <= 0.0 || taille.y <= 0.0 {
        return vec2(boite.x, boite.y.min(200.0));
    }
    let k = (boite.x / taille.x).min(boite.y / taille.y);
    (taille * k).max(Vec2::splat(1.0))
}

/// La hauteur d'une image ou d'un dessin dans sa carte, au plus : presque toute la conversation visible (elle se voit
/// d'un coup, sans défiler), 360 points au moins.
fn haut_max(ui: &egui::Ui) -> f32 {
    let visible = ui.clip_rect().height();
    if visible.is_finite() { (visible * 0.85).max(APERCU_HAUT) } else { APERCU_HAUT }
}

/// La vignette d'une page HTML (AP4) : egui n'a pas de moteur de page, la carte la dessine donc en clair, comme une
/// feuille de navigateur — son nom de fichier dans la barre, son titre, le début de son texte. Rend `true` si on clique
/// dessus.
fn vignette(ui: &mut egui::Ui, titre: &str, extrait: &str, nom: &str, clic: bool) -> bool {
    let dispo = ui.available_width();
    let l_texte = (dispo - 32.0).max(40.0);
    let p = ui.painter();
    let titre = if titre.is_empty() { nom } else { titre };
    let g_titre = mise_en_forme(p, titre, FontId::new(15.5, famille(natif::SEMI)), PAGE_ENCRE, l_texte, 2);
    let extrait = if extrait.is_empty() { "Cette page n'a pas de texte à montrer." } else { extrait };
    let g_extrait = mise_en_forme(p, extrait, FontId::proportional(12.5), PAGE_ENCRE_2, l_texte, 4);
    let g_nom = mise_en_forme(p, nom, FontId::proportional(11.0), PAGE_ENCRE_3, l_texte - 22.0, 1);
    const BARRE: f32 = 26.0;
    let h = BARRE + 14.0 + g_titre.size().y + 6.0 + g_extrait.size().y + 16.0;
    let (r, rep) = ui.allocate_exact_size(vec2(dispo, h), if clic { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    p.rect_filled(r, 6.0, Color32::WHITE);
    let barre = Rect::from_min_size(r.min, vec2(r.width(), BARRE));
    p.rect_filled(barre, CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }, PAGE_BARRE);
    p.hline(barre.x_range(), barre.bottom() - 0.5, Stroke::new(1.0, PAGE_FILET));
    let centre = pos2(barre.left() + 16.0, barre.center().y);
    p.text(centre, Align2::CENTER_CENTER, icones::GLOBE, FontId::new(11.0, famille(natif::ICONES)), PAGE_ENCRE_3);
    p.galley(pos2(barre.left() + 30.0, barre.center().y - g_nom.size().y / 2.0), g_nom, PAGE_ENCRE_3);
    let y = barre.bottom() + 14.0;
    let h_titre = g_titre.size().y;
    p.galley(pos2(r.left() + 16.0, y), g_titre, PAGE_ENCRE);
    p.galley(pos2(r.left() + 16.0, y + h_titre + 6.0), g_extrait, PAGE_ENCRE_2);
    p.rect_stroke(r, 6.0, Stroke::new(1.0, Color32::from_black_alpha(46)), StrokeKind::Inside);
    clic && rep.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Ouvrir en grand").clicked()
}

/// L'aperçu se lit, le dessin se dessine : un creux, un tourniquet, et `texte`.
fn attente(ui: &mut egui::Ui, texte: &str) {
    let t = teintes(ui);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 120.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 6.0, t.creux);
    let g = p.layout_no_wrap(texte.to_string(), FontId::proportional(12.5), t.encre_3);
    let x = r.center().x - (16.0 + 8.0 + g.size().x) / 2.0;
    p.galley(pos2(x + 24.0, r.center().y - g.size().y / 2.0), g, t.encre_3);
    let tourniquet = Rect::from_min_size(pos2(x, r.center().y - 8.0), vec2(16.0, 16.0));
    egui::Spinner::new().size(16.0).color(t.encre_3).paint_at(ui, tourniquet);
}

/// Ce qu'un aperçu dit quand il ne s'affiche pas (AP5), dans un creux de la carte.
fn avis(ui: &mut egui::Ui, probleme: &Probleme) {
    let t = teintes(ui);
    let (code, couleur) = probleme.icone();
    egui::Frame::new().fill(t.creux).corner_radius(8.0).inner_margin(Margin::symmetric(11, 9)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 9.0;
            ui.label(icone(code, 14.0).color(couleur));
            let message = RichText::new(probleme.message()).size(12.5).color(t.encre_2);
            ui.add(egui::Label::new(message).wrap());
        });
    });
}

// ---- Les dessins ---------------------------------------------------------------------------------------------------

/// Un dessin d'Eve, d'une image à l'autre : son SVG ([`svg_complet`]), son titre, ce qu'on en montre, et le rendu
/// demandé au fil.
struct Dessin {
    svg: Arc<str>,
    titre: String,
    etat: EtatDessin,
    /// La largeur, en pixels, du dernier rendu demandé : un autre ne se demande que s'il en faut un plus large, ou un
    /// deux fois plus étroit (les mipmaps réduisent le reste sans flou).
    demandee: Option<u32>,
    en_lecture: bool,
    vu: f64,
}

enum EtatDessin {
    Attente,
    Image { texture: egui::TextureHandle, intrinseque: Vec2 },
    Echec(Probleme),
}

impl Dessin {
    fn nouveau(svg: &str, temps: f64) -> Dessin {
        let svg = svg_complet(svg.trim());
        let etat = if svg.len() > DESSIN_MAX {
            EtatDessin::Echec(Probleme::DessinTropGros(svg.len()))
        } else {
            EtatDessin::Attente
        };
        let titre = titre_du_dessin(&svg);
        Dessin { svg: Arc::from(svg.as_ref()), titre, etat, demandee: None, en_lecture: false, vu: temps }
    }

    fn recevoir(&mut self, ctx: &egui::Context, cle: u64, rendu: DessinRendu) {
        self.en_lecture = false;
        self.etat = match rendu {
            Some((image, intrinseque)) => {
                let options = egui::TextureOptions {
                    mipmap_mode: Some(egui::TextureFilter::Linear),
                    ..egui::TextureOptions::LINEAR
                };
                let texture = ctx.load_texture(format!("dessin:{cle:016x}"), image, options);
                EtatDessin::Image { texture, intrinseque }
            }
            None => EtatDessin::Echec(Probleme::DessinIllisible),
        };
    }
}

/// La carte d'un dessin : son titre ; le dessin sur une feuille blanche (comme un aperçu : tout ce que Claude dessine
/// pour une page se lit, son texte noir compris), de toute la largeur de la carte, ses proportions gardées, presque
/// toute la conversation visible au plus ; « Ouvrir en grand » (le programme du poste, par un fichier du dossier
/// local) et « Enregistrer » (la boîte « Enregistrer sous » de Windows, en PNG ou en SVG). Le dessin se rend dans le
/// fil des aperçus, à la largeur où il s'affiche, quand il est à l'écran ou presque.
#[allow(clippy::too_many_arguments)]
fn carte_dessin(
    ui: &mut egui::Ui,
    cle: u64,
    d: &mut Dessin,
    lecture: &mut Lecture,
    local: &Path,
    enregistre: &Arc<Mutex<Option<(u64, String)>>>,
    id: egui::Id,
    encadre: bool,
) -> Option<Geste> {
    let (mut ouvrir, mut enregistrer, mut copier) = (false, false, false);
    let titre = d.titre.clone();
    let mut corps = |ui: &mut egui::Ui| {
        let dispo = ui.available_width();
        let taille = match &d.etat {
            EtatDessin::Image { intrinseque, .. } => remplir(*intrinseque, vec2(dispo, haut_max(ui))),
            _ => vec2(dispo, 120.0),
        };
        let place = Rect::from_min_size(ui.cursor().min, vec2(dispo, taille.y));
        if ui.is_rect_visible(place.expand(200.0)) && !d.en_lecture && !matches!(d.etat, EtatDessin::Echec(_)) {
            let limite = ui.input(|i| i.max_texture_side).min(TEXTURE_MAX);
            let voulue = palier(taille.x * ui.ctx().pixels_per_point(), limite);
            if d.demandee.is_none_or(|l| voulue > l || voulue * 2 < l) {
                d.demandee = Some(voulue);
                let svg = d.svg.clone();
                d.en_lecture =
                    lecture.dessiner(DemandeDeDessin { cle, svg, largeur: voulue, limite, ctx: ui.ctx().clone() });
                if !d.en_lecture {
                    d.etat = EtatDessin::Echec(Probleme::DessinIllisible);
                }
            }
        }
        match &d.etat {
            EtatDessin::Attente => attente(ui, "Le dessin s'affiche…"),
            EtatDessin::Image { texture, .. } => {
                ouvrir = image(ui, texture, taille, false, true, id);
                ui.add_space(10.0);
                // Dans une carte étroite (le tableau de bord), les boutons passent à la ligne.
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    ouvrir |= bouton_lien(ui, pictos::OUVRIR, "Ouvrir en grand").clicked();
                    enregistrer = bouton_lien(ui, pictos::ENREGISTRER, "Enregistrer").clicked();
                    copier = bouton_lien(ui, icones::COPIER, "Copier l'image").clicked();
                });
                let fait = enregistre.lock().unwrap_or_else(|e| e.into_inner()).clone();
                if let Some((_, message)) = fait.filter(|(c, _)| *c == cle) {
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(RichText::new(message).size(12.0).color(teintes(ui).encre_3)).wrap());
                }
            }
            EtatDessin::Echec(p) => {
                avis(ui, p);
                // Ce qui cloche se voit dans le SVG lui-même.
                if *p == Probleme::DessinIllisible {
                    ui.add_space(8.0);
                    bloc_brut(ui, &d.svg, id);
                }
            }
        }
    };
    if encadre {
        cadre(ui, pictos::DESSIN, &titre, "", None, corps);
    } else {
        corps(ui);
    }
    if enregistrer {
        enregistrer_le_dessin(&d.titre, d.svg.clone(), cle, enregistre.clone(), ui.ctx().clone());
    }
    if copier {
        copier_le_dessin(d.svg.clone(), cle, enregistre.clone(), ui.ctx().clone());
    }
    if !ouvrir {
        return None;
    }
    match dessin_dans_un_fichier(local, &d.titre, cle, &d.svg) {
        Ok(chemin) => Some(Geste { action: "ouvrir_fichier", valeur: Value::String(chemin.display().to_string()) }),
        Err(e) => {
            let message = format!("Le dessin ne s'ouvre pas : {e}");
            *enregistre.lock().unwrap_or_else(|e| e.into_inner()) = Some((cle, message));
            None
        }
    }
}

/// Une carte épinglée au tableau de bord, dans la conversation : ce qui s'est fait ; dans le thème clair, où le tableau
/// de bord se voit, de quoi aller la voir (le geste `voir_epingle`, que la fenêtre prend pour elle).
fn epingle(ui: &mut egui::Ui, id: &str, retiree: bool) -> Option<Geste> {
    let t = teintes(ui);
    let texte = if retiree { "Retirée du tableau de bord." } else { "Épinglée au tableau de bord." };
    ui.add(egui::Label::new(RichText::new(texte).size(13.5).color(t.encre)).wrap());
    if retiree {
        return None;
    }
    if ui.visuals().dark_mode {
        let ou = "Le tableau de bord se voit dans le thème « Constructo AI » (Réglages, Thème).";
        ui.add(egui::Label::new(RichText::new(ou).size(12.0).color(t.encre_3)).wrap());
        return None;
    }
    ui.add_space(4.0);
    let voir = bouton_lien(ui, pictos::EPINGLE, "Voir au tableau de bord").clicked();
    voir.then(|| Geste { action: "voir_epingle", valeur: Value::String(id.to_string()) })
}

/// La largeur d'un rendu, en pixels : celle qu'il prend à l'écran, arrondie au palier du dessus, dans la limite des
/// textures.
fn palier(pixels: f32, limite: usize) -> u32 {
    let pixels = pixels.max(1.0).ceil() as u32;
    (pixels.div_ceil(PALIER) * PALIER).min(limite.max(1) as u32)
}

/// Un dessin d'Eve en pixels, pour l'écran : `largeur` pixels de large (sa hauteur suit), le grand côté dans `limite` ;
/// et sa taille d'origine.
fn dessin_en_pixels(svg: &str, largeur: u32, limite: usize) -> Result<(egui::ColorImage, Vec2), Probleme> {
    let (toile, intrinseque) = toile_du_dessin(svg, largeur as f32, limite)?;
    let taille = [toile.width() as usize, toile.height() as usize];
    Ok((egui::ColorImage::from_rgba_premultiplied(taille, toile.data()), intrinseque))
}

/// Un dessin rendu par resvg sur une feuille blanche : `largeur` pixels de large au plus, le grand côté dans `limite`,
/// ses proportions gardées ; son texte dans les polices du poste, Segoe UI d'office.
fn toile_du_dessin(svg: &str, largeur: f32, limite: usize) -> Result<(resvg::tiny_skia::Pixmap, Vec2), Probleme> {
    use resvg::{tiny_skia, usvg};
    let options = options_svg(Some(POLICE_DES_DESSINS));
    let arbre = usvg::Tree::from_str(svg, &options).map_err(|_| Probleme::DessinIllisible)?;
    let (l, h) = (arbre.size().width(), arbre.size().height());
    let k = (largeur / l).min(limite as f32 / l.max(h));
    let cote = |x: f32| ((x * k).round() as u32).clamp(1, limite as u32);
    let (lp, hp) = (cote(l), cote(h));
    let mut toile = tiny_skia::Pixmap::new(lp, hp).ok_or(Probleme::DessinIllisible)?;
    toile.fill(tiny_skia::Color::WHITE);
    resvg::render(&arbre, tiny_skia::Transform::from_scale(lp as f32 / l, hp as f32 / h), &mut toile.as_mut());
    Ok((toile, vec2(l, h)))
}

/// Le SVG d'un dessin tel que le lisent les programmes du poste : Claude l'écrit souvent comme dans une page web, sans
/// l'espace de noms du SVG, sans lequel un navigateur n'ouvre pas le fichier (resvg s'en passe) ; il s'ajoute, et
/// celui de xlink si le dessin s'en sert.
fn svg_complet(svg: &str) -> Cow<'_, str> {
    let Some(debut) = svg.find("<svg") else { return Cow::Borrowed(svg) };
    let balise = &svg[debut..svg[debut..].find('>').map_or(svg.len(), |i| debut + i)];
    let mut ajout = String::new();
    if !balise.contains("xmlns=") {
        ajout.push_str(" xmlns=\"http://www.w3.org/2000/svg\"");
    }
    if svg.contains("xlink:") && !balise.contains("xmlns:xlink") {
        ajout.push_str(" xmlns:xlink=\"http://www.w3.org/1999/xlink\"");
    }
    if ajout.is_empty() {
        return Cow::Borrowed(svg);
    }
    let apres = debut + "<svg".len();
    Cow::Owned(format!("{}{ajout}{}", &svg[..apres], &svg[apres..]))
}

/// Le titre d'un dessin : son premier « <title> », sinon « Dessin ».
pub(crate) fn titre_du_dessin(svg: &str) -> String {
    let titre = svg
        .find("<title")
        .and_then(|i| svg[i..].find('>').map(|j| i + j + 1))
        .and_then(|debut| svg[debut..].find("</title").map(|fin| nettoyer(&svg[debut..debut + fin])))
        .unwrap_or_default();
    if titre.is_empty() { "Dessin".to_string() } else { titre }
}

/// Un nom de fichier tiré du titre d'un dessin : sans ce que Windows refuse dans un nom, 60 caractères au plus ;
/// « Dessin » s'il ne reste rien.
fn nom_de_fichier(titre: &str) -> String {
    let propre: String =
        titre.chars().map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { ' ' } else { c }).collect();
    let nom: String = propre.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(60).collect();
    let nom = nom.trim_end_matches(['.', ' ']);
    if nom.is_empty() || nom_reserve(nom) { "Dessin".to_string() } else { nom.to_string() }
}

/// Le dessin dans un fichier du dossier local (`dessins\`), pour l'ouvrir en grand dans le programme du poste : son nom
/// vient de son titre et de son empreinte (le même dessin, le même fichier).
fn dessin_dans_un_fichier(local: &Path, titre: &str, cle: u64, svg: &str) -> std::io::Result<PathBuf> {
    let dossier = local.join("dessins");
    std::fs::create_dir_all(&dossier)?;
    let chemin = dossier.join(format!("{} {:06x}.svg", nom_de_fichier(titre), cle & 0xFF_FFFF));
    std::fs::write(&chemin, svg)?;
    Ok(chemin)
}

/// « Enregistrer » : la boîte « Enregistrer sous » de Windows, dans son fil, attachée à la fenêtre d'Eve ; le dessin
/// s'écrit en PNG ou en SVG, selon le nom choisi ; ce qu'il en est se dit sous lui. Annulée, elle ne dit rien.
fn enregistrer_le_dessin(
    titre: &str,
    svg: Arc<str>,
    cle: u64,
    avis: Arc<Mutex<Option<(u64, String)>>>,
    ctx: egui::Context,
) {
    let nom = nom_de_fichier(titre);
    let proprietaire = natif::pieces::fenetre_d_eve();
    thread::spawn(move || {
        let message = match systeme::enregistrer_sous(proprietaire, &nom) {
            Ok(None) => return,
            Ok(Some(chemin)) => match panic::catch_unwind(AssertUnwindSafe(|| ecrire_le_dessin(&chemin, &svg))) {
                Ok(Ok(())) => format!("Enregistré : {}", chemin.display()),
                Ok(Err(e)) => format!("Le dessin ne s'enregistre pas : {e}"),
                Err(_) => "Le dessin ne s'enregistre pas : son SVG ne se lit pas.".to_string(),
            },
            Err(e) => format!("La boîte « Enregistrer sous » ne s'ouvre pas : {e}"),
        };
        *avis.lock().unwrap_or_else(|e| e.into_inner()) = Some((cle, message));
        ctx.request_repaint();
    });
}

/// « Copier l'image » : le dessin rendu comme pour « Enregistrer » en PNG ([`PNG_COTE`] pixels, sur du blanc), dans
/// un fil, puis au presse-papiers en image (il se colle dans un courriel, Word ou Paint) ; ce qu'il en est se dit sous
/// lui.
fn copier_le_dessin(svg: Arc<str>, cle: u64, avis: Arc<Mutex<Option<(u64, String)>>>, ctx: egui::Context) {
    thread::spawn(move || {
        let rendu = panic::catch_unwind(AssertUnwindSafe(|| toile_du_dessin(&svg, f32::INFINITY, PNG_COTE)));
        let message = match rendu {
            Ok(Ok((toile, _))) => {
                let taille = [toile.width() as usize, toile.height() as usize];
                ctx.copy_image(egui::ColorImage::from_rgba_premultiplied(taille, toile.data()));
                "Image copiée : colle-la avec Ctrl+V.".to_string()
            }
            Ok(Err(p)) => format!("Le dessin ne se copie pas : {}", p.message()),
            Err(_) => "Le dessin ne se copie pas : son SVG ne se lit pas.".to_string(),
        };
        *avis.lock().unwrap_or_else(|e| e.into_inner()) = Some((cle, message));
        ctx.request_repaint();
    });
}

/// Le fichier choisi pour enregistrer un dessin, avec son extension : celle qu'on a tapée (.png ou .svg), sinon celle
/// du type choisi dans la boîte (1 : PNG, 2 : SVG).
fn avec_extension(chemin: PathBuf, type_choisi: u32) -> PathBuf {
    let connue = chemin.extension().is_some_and(|e| e.eq_ignore_ascii_case("png") || e.eq_ignore_ascii_case("svg"));
    if connue {
        return chemin;
    }
    let mut nom = chemin.into_os_string();
    nom.push(if type_choisi == 2 { ".svg" } else { ".png" });
    PathBuf::from(nom)
}

/// Le dessin dans `chemin` : en SVG si son nom finit par « .svg », en PNG sinon.
fn ecrire_le_dessin(chemin: &Path, svg: &str) -> Result<(), String> {
    let en_svg = chemin.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let octets = if en_svg { svg.as_bytes().to_vec() } else { png_du_dessin(svg)? };
    std::fs::write(chemin, octets).map_err(|e| e.to_string())
}

/// Le dessin en PNG, sur du blanc, [`PNG_COTE`] pixels de grand côté : de quoi l'imprimer ou l'envoyer. Sur du blanc,
/// chaque pixel est opaque : ses couleurs prémultipliées sont ses couleurs.
fn png_du_dessin(svg: &str) -> Result<Vec<u8>, String> {
    let (toile, _) = toile_du_dessin(svg, f32::INFINITY, PNG_COTE).map_err(|p| p.message())?;
    png_rgba(toile.width(), toile.height(), toile.data())
}

/// Une image en PNG : `rgba`, ses pixels rangée par rangée, quatre octets chacun, non prémultipliés. Pour un dessin
/// enregistré et pour une capture d'écran collée dans la fenêtre.
pub(crate) fn png_rgba(largeur: u32, hauteur: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut octets = Vec::new();
    {
        let mut e = png::Encoder::new(&mut octets, largeur, hauteur);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut w = e.write_header().map_err(|e| e.to_string())?;
        w.write_image_data(rgba).map_err(|e| e.to_string())?;
        w.finish().map_err(|e| e.to_string())?;
    }
    Ok(octets)
}

// ---- Le cadre et les autres cartes ---------------------------------------------------------------------------------

/// Le cadre commun des cartes (K2) : l'icône dans un carré arrondi, le titre, au besoin une précision à droite, puis
/// le contenu. `copie` : le JSON d'une carte qui se copie ([`en_texte`]) — une icône au bout de la tête.
fn cadre<R>(
    ui: &mut egui::Ui,
    code: char,
    titre: &str,
    precision: &str,
    copie: Option<&str>,
    contenu: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let t = teintes(ui);
    egui::Frame::new()
        .fill(t.fond)
        .stroke((1.0, t.bord))
        .corner_radius(10.0)
        .inner_margin(Margin { left: 12, right: 12, top: 10, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            tete(ui, code, titre, precision, copie);
            ui.add_space(9.0);
            contenu(ui)
        })
        .inner
}

/// Le côté de l'icône « Copier » d'une tête de carte.
const COPIE: f32 = 24.0;

/// La tête d'une carte : la tuile, le titre (sur plusieurs lignes s'il le faut), la précision ; au bout, l'icône
/// « Copier » d'une carte qui se copie (`copie`, son JSON) : son texte au presse-papiers, et une coche un moment.
fn tete(ui: &mut egui::Ui, code: char, titre: &str, precision: &str, copie: Option<&str>) {
    let t = teintes(ui);
    let dispo = ui.available_width();
    let p = ui.painter();
    let g_precision =
        (!precision.is_empty()).then(|| p.layout_no_wrap(precision.to_string(), FontId::proportional(11.5), t.encre_3));
    let l_copie = if copie.is_some() { COPIE + 6.0 } else { 0.0 };
    let reserve = g_precision.as_ref().map_or(0.0, |g| g.size().x + 12.0) + l_copie;
    let l_titre = (dispo - TUILE - 10.0 - reserve).max(60.0);
    let g_titre = p.layout(titre.to_string(), FontId::new(14.0, famille(natif::SEMI)), t.encre, l_titre);
    let ligne = g_titre.rows.first().map_or(18.0, |l| l.rect().height());
    let decalage = ((TUILE - ligne) / 2.0).max(0.0);
    let h = TUILE.max(decalage + g_titre.size().y);
    let (r, _) = ui.allocate_exact_size(vec2(dispo, h), Sense::hover());
    let p = ui.painter();
    let tuile = Rect::from_min_size(r.min, Vec2::splat(TUILE));
    p.rect(
        tuile,
        7.0,
        melange(t.fond, t.accent, 0.16),
        Stroke::new(1.0, melange(t.fond, t.accent, 0.34)),
        StrokeKind::Inside,
    );
    let icone = FontId::new(13.0, famille(natif::ICONES));
    p.text(tuile.center(), Align2::CENTER_CENTER, code, icone.clone(), t.accent);
    p.galley(pos2(r.left() + TUILE + 10.0, r.top() + decalage), g_titre, t.encre);
    if let Some(g) = g_precision {
        p.galley(pos2(r.right() - l_copie - g.size().x, tuile.center().y - g.size().y / 2.0), g, t.encre_3);
    }
    let Some(json) = copie else { return };
    let place = Rect::from_center_size(pos2(r.right() - COPIE / 2.0, tuile.center().y), Vec2::splat(COPIE));
    let rep = ui.interact(place, ui.scope_id().with(("copier-la-carte", json)), Sense::click());
    let copie = natif::copier_au_clic(ui, &rep, rep.id, || en_texte(json).unwrap_or_default());
    let p = ui.painter();
    if rep.hovered() {
        p.rect_filled(place, 6.0, t.accent.gamma_multiply(0.13));
    }
    let (code, encre) = if copie { (icones::OUI, t.accent) } else { (icones::COPIER, t.encre_3) };
    let encre = if rep.hovered() { t.accent } else { encre };
    p.text(place.center(), Align2::CENTER_CENTER, code, icone, encre);
    let bulle = if copie { "Copié" } else { "Copier (un tableau se colle dans Excel)" };
    rep.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(bulle);
}

/// Ce qu'une carte dit quand elle n'a rien.
fn vide(ui: &mut egui::Ui, texte: &str) {
    ui.label(RichText::new(texte).size(13.0).color(teintes(ui).encre_3));
}

fn famille(nom: &str) -> FontFamily {
    FontFamily::Name(nom.into())
}

/// Un texte mis en forme sur `largeur`, `lignes` lignes au plus (la dernière finie par « … »).
fn mise_en_forme(
    p: &egui::Painter,
    texte: &str,
    police: FontId,
    encre: Color32,
    largeur: f32,
    lignes: usize,
) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(texte.to_string(), TextFormat::simple(police, encre));
    job.wrap.max_width = largeur;
    job.wrap.max_rows = lignes;
    job.wrap.overflow_character = Some('…');
    p.layout_job(job)
}

/// Un bouton de carte (« Ouvrir dans Outlook », « Ouvrir en grand ») : cerné de la couleur d'accent, comme dans la
/// page, aux mesures des petits boutons de la fenêtre.
fn bouton_lien(ui: &mut egui::Ui, code: char, texte: &str) -> egui::Response {
    let p = ui.painter();
    let g_icone = p.layout_no_wrap(code.to_string(), FontId::new(12.5, famille(natif::ICONES)), Color32::PLACEHOLDER);
    let g_texte = p.layout_no_wrap(texte.to_string(), FontId::new(12.5, famille(natif::SEMI)), Color32::PLACEHOLDER);
    let l_icone = g_icone.size().x;
    let taille = vec2(10.0 + l_icone + 7.0 + g_texte.size().x + 11.0, g_texte.size().y + 11.0);
    let (r, rep) = ui.allocate_exact_size(taille, Sense::click());
    if ui.is_rect_visible(r) {
        let accent = teintes(ui).accent;
        let (survol, enfonce) = (rep.hovered(), rep.is_pointer_button_down_on());
        let fond = match (enfonce, survol) {
            (true, _) => accent.gamma_multiply(0.24),
            (false, true) => accent.gamma_multiply(0.13),
            _ => Color32::TRANSPARENT,
        };
        let bord = accent.gamma_multiply(if survol { 0.85 } else { 0.5 });
        // Au survol, l'encre s'éclaircit sur le sombre, et fonce sur le clair.
        let vers = if ui.visuals().dark_mode { Color32::WHITE } else { Color32::BLACK };
        let encre = if survol { melange(accent, vers, 0.3) } else { accent };
        let p = ui.painter();
        p.rect(r, 8.0, fond, Stroke::new(1.0, bord), StrokeKind::Inside);
        let y = r.center().y;
        let (h_icone, h_texte) = (g_icone.size().y, g_texte.size().y);
        p.galley(pos2(r.left() + 10.0, y - h_icone / 2.0), g_icone, encre);
        p.galley(pos2(r.left() + 10.0 + l_icone + 7.0, y - h_texte / 2.0), g_texte, encre);
    }
    rep.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// L'agenda (K3) : les jours en petites capitales ; chaque événement sur trois colonnes — la pastille de sa sorte,
/// l'heure, le titre et le détail —, un filet entre deux.
fn agenda(ui: &mut egui::Ui, a: &Agenda, id: egui::Id) {
    if a.jours.is_empty() {
        vide(ui, "Rien au calendrier.");
        return;
    }
    ui.spacing_mut().item_spacing.y = 0.0;
    let t = teintes(ui);
    for (i, j) in a.jours.iter().enumerate() {
        if !j.jour.is_empty() {
            if i > 0 {
                ui.add_space(12.0);
            }
            ui.label(semi(j.jour.to_uppercase(), 10.5).extra_letter_spacing(0.8).color(t.encre_3));
            ui.add_space(2.0);
        }
        if j.evenements.is_empty() {
            ui.add_space(4.0);
            vide(ui, "Rien de prévu.");
            continue;
        }
        for (k, e) in j.evenements.iter().enumerate() {
            evenement(ui, e, k == 0, id.with(("pastille", i, k)));
        }
    }
}

/// Un événement de l'agenda : la pastille centrée sur la première ligne du titre, l'heure alignée sur elle.
fn evenement(ui: &mut egui::Ui, e: &Evenement, premier: bool, id: egui::Id) {
    const L_PASTILLE: f32 = 10.0;
    const L_HEURE: f32 = 56.0;
    const ECART: f32 = 10.0;
    let dispo = ui.available_width();
    let x_heure = L_PASTILLE + ECART;
    let x_corps = x_heure + L_HEURE + ECART;
    let l_corps = (dispo - x_corps).max(60.0);
    let ferie = e.sorte == Some(SorteEv::Ferie);
    let t = teintes(ui);
    let (police, encre) = if ferie {
        (FontId::proportional(14.0), t.encre_2)
    } else {
        (FontId::new(14.0, famille(natif::SEMI)), t.encre)
    };
    let p = ui.painter();
    let g_titre = p.layout(e.titre.clone(), police, encre, l_corps);
    let g_detail =
        (!e.detail.is_empty()).then(|| p.layout(e.detail.clone(), FontId::proportional(12.5), t.encre_2, l_corps));
    // Les chiffres de Segoe UI ont tous la même chasse : les heures s'alignent sans police à chasse fixe.
    let g_heure = p.layout(e.heure.clone(), FontId::proportional(13.0), t.encre_2, L_HEURE);
    let h_titre = g_titre.size().y;
    let h_corps = h_titre + g_detail.as_ref().map_or(0.0, |g| 1.0 + g.size().y);
    let h = h_corps.max(g_heure.size().y) + 2.0 * 7.0;
    let (r, _) = ui.allocate_exact_size(vec2(dispo, h), Sense::hover());
    let p = ui.painter();
    if !premier {
        p.hline(r.x_range(), r.top() + 0.5, Stroke::new(1.0, t.filet));
    }
    let haut = r.top() + 7.0;
    let ligne = g_titre.rows.first().map_or(18.0, |l| l.rect().height());
    let centre = pos2(r.left() + L_PASTILLE / 2.0, haut + ligne / 2.0);
    pastille(p, centre, e.sorte, t);
    let ligne_heure = g_heure.rows.first().map_or(14.0, |l| l.rect().height());
    p.galley(pos2(r.left() + x_heure, haut + (ligne - ligne_heure) / 2.0), g_heure, t.encre_2);
    p.galley(pos2(r.left() + x_corps, haut), g_titre, encre);
    if let Some(g) = g_detail {
        p.galley(pos2(r.left() + x_corps, haut + h_titre + 1.0), g, t.encre_2);
    }
    if let Some(s) = e.sorte {
        let zone = Rect::from_center_size(centre, vec2(16.0, 16.0));
        let rep = ui.interact(zone, id, Sense::hover());
        rep.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, s.nom()));
        let _ = rep.on_hover_text(s.nom());
    }
}

/// La pastille d'une sorte d'événement : un carré de sa couleur ; une échéance, un rond ; un jalon, cerclé d'un double
/// anneau ; sans sorte, un carré vide.
fn pastille(p: &egui::Painter, centre: Pos2, sorte: Option<SorteEv>, t: &Teintes) {
    let r = Rect::from_center_size(centre, vec2(10.0, 10.0));
    match sorte {
        None => {
            p.rect_stroke(r, 2.0, Stroke::new(1.5, t.filet_fort), StrokeKind::Inside);
        }
        Some(SorteEv::Echeance) => {
            p.circle_filled(centre, 5.0, t.s_attente);
        }
        Some(SorteEv::Gros) => {
            p.rect_filled(r, 2.0, t.s_confirme);
            p.rect_stroke(r.expand(2.75), 4.5, Stroke::new(1.5, t.s_confirme), StrokeKind::Middle);
        }
        Some(s) => {
            let couleur = match s {
                SorteEv::Attente => t.s_attente,
                SorteEv::Appel => t.s_appel,
                SorteEv::Ferie => t.s_ferie,
                _ => t.s_confirme,
            };
            p.rect_filled(r, 2.0, couleur);
        }
    }
}

/// Le brouillon (K5) : à qui, l'objet, quatre lignes de l'extrait au plus ; « Ouvrir dans Outlook » s'il a son id.
fn brouillon(ui: &mut egui::Ui, b: &Brouillon) -> Option<Geste> {
    let t = teintes(ui);
    let champs: Vec<(&str, &str, bool)> = [("À", b.a.as_str(), false), ("Objet", b.objet.as_str(), true)]
        .into_iter()
        .filter(|c| !c.1.is_empty())
        .collect();
    if !champs.is_empty() {
        let etiquette = |p: &egui::Painter, texte: &str| {
            let format = TextFormat {
                font_id: FontId::new(10.5, famille(natif::SEMI)),
                color: t.encre_3,
                extra_letter_spacing: 0.6,
                ..Default::default()
            };
            p.layout_job(LayoutJob::single_section(texte.to_uppercase(), format))
        };
        let l_etiquette = champs.iter().map(|c| etiquette(ui.painter(), c.0).size().x).fold(0.0, f32::max) + 12.0;
        for (nom, valeur, gras) in champs {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let g = etiquette(ui.painter(), nom);
                let (r, _) = ui.allocate_exact_size(vec2(l_etiquette, 19.0), Sense::hover());
                ui.painter().galley(pos2(r.left(), r.top() + 3.0), g, t.encre_3);
                let texte = if gras { semi(valeur, 13.5) } else { RichText::new(valeur).size(13.5) };
                ui.add(egui::Label::new(texte.color(t.encre)).wrap());
            });
            ui.add_space(2.0);
        }
        ui.add_space(6.0);
    }
    if !b.extrait.is_empty() {
        let dispo = ui.available_width();
        let p = ui.painter();
        let g = mise_en_forme(p, &en_lignes(&b.extrait), FontId::proportional(13.0), t.encre_2, dispo - 12.0, 4);
        let (r, _) = ui.allocate_exact_size(vec2(dispo, g.size().y + 2.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(Rect::from_min_size(r.min, vec2(2.0, r.height())), 1.0, t.filet_fort);
        p.galley(pos2(r.left() + 12.0, r.top() + 1.0), g, t.encre_2);
    }
    if b.id.is_empty() {
        return None;
    }
    ui.add_space(10.0);
    bouton_lien(ui, pictos::OUVRIR, "Ouvrir dans Outlook")
        .clicked()
        .then(|| Geste { action: "ouvrir_brouillon", valeur: Value::String(b.id.clone()) })
}

/// Une carte illisible (K6), un dessin qui ne se lit pas : son texte tel quel, en bloc brut, défilant au-delà de 224
/// points — le seul défilement dans une carte : ce texte ne sert qu'à voir ce qui cloche.
fn bloc_brut(ui: &mut egui::Ui, json: &str, id: egui::Id) {
    let t = teintes(ui);
    egui::Frame::new()
        .fill(t.brut)
        .stroke((1.0, t.filet))
        .corner_radius(8.0)
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Sa hauteur, même au bas d'une longue conversation : dans la conversation qui défile, egui ne donne à une
            // zone qui défile que la place restée visible au-dessus d'elle, et l'écrasait à 64 points.
            egui::ScrollArea::vertical().id_salt(id.with("brut")).max_height(224.0).min_scrolled_height(224.0).show(
                ui,
                |ui| {
                    let texte = RichText::new(json.trim()).monospace().size(11.5).color(t.encre_2);
                    ui.add(egui::Label::new(texte).wrap());
                },
            );
        });
}

/// Une carte ou un dessin qu'Eve écrit encore (F11) : un cadre pointillé, et `texte` (« Carte en préparation… »).
fn en_preparation(ui: &mut egui::Ui, texte: &str) {
    let t = teintes(ui);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
    let p = ui.painter();
    p.extend(egui::Shape::dashed_line(&contour_arrondi(r.shrink(0.5), 10.0), (1.0, t.filet_fort), 5.0, 4.0));
    let x = r.left() + 14.0;
    let police = FontId::new(13.0, famille(natif::SEMI));
    p.text(pos2(x + 24.0, r.center().y), Align2::LEFT_CENTER, texte, police, t.encre_3);
    let tourniquet = Rect::from_min_size(pos2(x, r.center().y - 7.0), vec2(14.0, 14.0));
    egui::Spinner::new().size(14.0).color(t.encre_3).paint_at(ui, tourniquet);
}

/// Le contour d'un rectangle aux coins arrondis, en points : pour le tracer en pointillé.
fn contour_arrondi(r: Rect, rayon: f32) -> Vec<Pos2> {
    let rayon = rayon.min(r.width() / 2.0).min(r.height() / 2.0);
    let coins = [
        (r.left_top() + vec2(rayon, rayon), 180.0f32),
        (r.right_top() + vec2(-rayon, rayon), 270.0),
        (r.right_bottom() + vec2(-rayon, -rayon), 0.0),
        (r.left_bottom() + vec2(rayon, -rayon), 90.0),
    ];
    let mut points: Vec<Pos2> = coins
        .iter()
        .flat_map(|(c, debut)| {
            (0..=8).map(move |i| {
                let a = (debut + 90.0 * i as f32 / 8.0).to_radians();
                *c + vec2(a.cos(), a.sin()) * rayon
            })
        })
        .collect();
    points.push(points[0]);
    points
}

// ---- Le tableau ----------------------------------------------------------------------------------------------------

/// Les largeurs naturelles des colonnes d'un tableau, marges comprises : au plus étroit (le mot le plus long, ou toute
/// la cellule si elle ne se coupe pas) et sur une ligne.
struct Mesures {
    ppp: f32,
    min: Vec<f32>,
    max: Vec<f32>,
}

/// Le tableau mis en page pour une largeur : les largeurs des colonnes, et chaque cellule déjà mise en forme.
struct MiseEnPage {
    ppp: f32,
    dispo: f32,
    largeurs: Vec<f32>,
    entete: Vec<Cellule>,
    h_entete: f32,
    lignes: Vec<Rangee>,
    pieds: Vec<Rangee>,
}

struct Cellule {
    galley: Arc<Galley>,
    droite: bool,
}

struct Rangee {
    cellules: Vec<Cellule>,
    hauteur: f32,
    /// Une note au pied : sa seule case prend toute la largeur ([`est_une_note`]).
    note: bool,
}

fn police_corps() -> FontId {
    FontId::proportional(13.0)
}

fn police_pied() -> FontId {
    FontId::new(13.0, famille(natif::SEMI))
}

fn police_note() -> FontId {
    FontId::proportional(12.5)
}

/// Une rangée du pied qui n'a qu'une case, dans un tableau qui en a plusieurs : une note (« Source : … », « Au-delà du
/// lundi, aucune prévision »), que Claude y met souvent. Elle prend toute la largeur du tableau : dans la première
/// colonne seule, une phrase s'y étirait sur cinq lignes et allongeait la carte d'autant.
fn est_une_note(rangee: &[String], n: usize) -> bool {
    n > 1 && rangee.len() == 1
}

fn police_entete() -> FontId {
    FontId::new(10.5, famille(natif::SEMI))
}

/// Une cellule mise en forme : sur `largeur` (l'infini : sur une ligne), l'espacement de ses lettres donné.
fn cellule(
    p: &egui::Painter,
    texte: &str,
    police: FontId,
    encre: Color32,
    espacement: f32,
    largeur: f32,
) -> Arc<Galley> {
    let format = TextFormat { font_id: police, color: encre, extra_letter_spacing: espacement, ..Default::default() };
    let mut job = LayoutJob::single_section(texte.to_string(), format);
    job.wrap.max_width = largeur;
    p.layout_job(job)
}

/// La marge à droite de la colonne `k` sur `n` : aucune pour la dernière.
fn marge(k: usize, n: usize) -> f32 {
    if k + 1 < n { MARGE_COLONNE } else { 0.0 }
}

/// Les largeurs naturelles des colonnes : l'en-tête ne se coupe pas, une cellule alignée à droite non plus ; le
/// reste se coupe entre ses mots. Une note au pied n'y compte pas : elle prend toute la largeur.
fn mesurer(p: &egui::Painter, t: &Tableau, ppp: f32) -> Mesures {
    let n = t.n;
    let (mut min, mut max) = (vec![0.0f32; n], vec![0.0f32; n]);
    let mut voir = |k: usize, texte: &str, police: FontId, espacement: f32, insecable: bool| {
        let largeur = |x: &str| cellule(p, x, police.clone(), Color32::PLACEHOLDER, espacement, f32::INFINITY).size().x;
        let entier = largeur(texte);
        let mot = if insecable { entier } else { texte.split_whitespace().map(largeur).fold(0.0, f32::max) };
        max[k] = max[k].max(entier);
        min[k] = min[k].max(mot);
    };
    for (k, c) in t.colonnes.iter().enumerate() {
        voir(k, &c.to_uppercase(), police_entete(), 0.6, true);
    }
    let pieds = t.pieds.iter().filter(|l| !est_une_note(l, n));
    let rangees = t.lignes.iter().map(|l| (l, police_corps())).chain(pieds.map(|l| (l, police_pied())));
    for (l, police) in rangees {
        for (k, v) in l.iter().enumerate() {
            voir(k, v, police.clone(), 0.0, t.droite(k, v));
        }
    }
    for k in 0..n {
        min[k] = (min[k] + marge(k, n)).ceil();
        max[k] = (max[k] + marge(k, n)).ceil();
    }
    Mesures { ppp, min, max }
}

/// Les largeurs des colonnes dans `dispo`, comme un tableau HTML à `width: 100%` : tout tient sur une ligne, la place
/// qui reste se partage selon la largeur de chacune ; sinon les colonnes se resserrent, chacune d'autant plus qu'elle
/// le peut ; au-delà de leur plus étroit, le tableau défile en largeur dans sa carte.
fn largeurs(min: &[f32], max: &[f32], dispo: f32) -> Vec<f32> {
    let (smin, smax): (f32, f32) = (min.iter().sum(), max.iter().sum());
    if smax <= dispo {
        if smax <= 0.0 {
            return vec![dispo / min.len().max(1) as f32; min.len()];
        }
        max.iter().map(|m| m * dispo / smax).collect()
    } else if smin < dispo {
        let t = (dispo - smin) / (smax - smin);
        min.iter().zip(max).map(|(a, b)| a + (b - a) * t).collect()
    } else {
        min.to_vec()
    }
}

/// Le tableau mis en page pour `dispo` : chaque cellule mise en forme sur sa colonne, les hauteurs des rangées.
fn mettre_en_page(p: &egui::Painter, t: &Tableau, m: &Mesures, dispo: f32) -> MiseEnPage {
    let n = t.n;
    let largeurs = largeurs(&m.min, &m.max, dispo);
    let mise = |k: usize, texte: &str, police: FontId, encre: Color32, espacement: f32| {
        let droite = t.droite(k, texte);
        let l = if droite { f32::INFINITY } else { (largeurs[k] - marge(k, n)).max(8.0) };
        Cellule { galley: cellule(p, texte, police, encre, espacement, l), droite }
    };
    let rangee = |l: &Vec<String>, police: FontId, encre: Color32, marges: f32| {
        let cellules: Vec<Cellule> =
            (0..n).map(|k| mise(k, l.get(k).map_or("", String::as_str), police.clone(), encre, 0.0)).collect();
        let hauteur = cellules.iter().map(|c| c.galley.size().y).fold(0.0, f32::max) + marges;
        Rangee { cellules, hauteur, note: false }
    };
    // Une note se coupe sur la largeur visible de la carte : elle se lit sans faire défiler un tableau trop large.
    let note = |l: &Vec<String>, encre: Color32| {
        let texte = l.first().map_or("", String::as_str);
        let droite = ressemble_a_un_nombre(texte);
        let galley = cellule(p, texte, police_note(), encre, 0.0, if droite { f32::INFINITY } else { dispo.max(8.0) });
        let hauteur = galley.size().y + MARGE_RANGEE + 3.0;
        Rangee { cellules: vec![Cellule { galley, droite }], hauteur, note: true }
    };
    // Les cellules se mettent en page sans couleur : elles prennent celles du thème quand elles se peignent.
    let encre = Color32::PLACEHOLDER;
    let entete: Vec<Cellule> = if t.colonnes.is_empty() {
        Vec::new()
    } else {
        (0..n)
            .map(|k| {
                let c = t.colonnes.get(k).map_or(String::new(), |c| c.to_uppercase());
                mise(k, &c, police_entete(), encre, 0.6)
            })
            .collect()
    };
    let h_entete =
        entete.iter().map(|c| c.galley.size().y).fold(0.0, f32::max) + if entete.is_empty() { 0.0 } else { 7.0 };
    let lignes = t.lignes.iter().map(|l| rangee(l, police_corps(), encre, 2.0 * MARGE_RANGEE)).collect();
    let pieds = t
        .pieds
        .iter()
        .map(|l| if est_une_note(l, n) { note(l, encre) } else { rangee(l, police_pied(), encre, MARGE_RANGEE + 3.0) })
        .collect();
    MiseEnPage { ppp: m.ppp, dispo, largeurs, entete, h_entete, lignes, pieds }
}

/// Le tableau (K4) : l'en-tête en petites capitales ; les nombres à droite ; le pied en gras sous un filet, une note
/// sur toute la largeur. Toutes ses rangées, toujours : c'est la conversation qui défile. Seul le défilement en
/// largeur reste dans la carte, quand les colonnes ne tiennent pas ; l'en-tête le suit.
fn tableau(
    ui: &mut egui::Ui,
    t: &Tableau,
    mesures: &mut Option<Mesures>,
    mises_en_page: &mut Vec<MiseEnPage>,
    id: egui::Id,
) {
    if t.n == 0 {
        vide(ui, "Tableau vide.");
        return;
    }
    let ppp = ui.ctx().pixels_per_point();
    let dispo = ui.available_width().floor();
    if mesures.as_ref().is_none_or(|m| m.ppp != ppp) {
        *mesures = Some(mesurer(ui.painter(), t, ppp));
        mises_en_page.clear();
    }
    let Some(m) = mesures.as_ref() else { return };
    // Quelques largeurs gardées : la même carte peut se dessiner à deux largeurs (une fenêtre qu'on étire).
    let mp = match mises_en_page.iter().position(|x| x.dispo == dispo && x.ppp == ppp) {
        Some(i) => &mises_en_page[i],
        None => {
            if mises_en_page.len() >= 3 {
                mises_en_page.remove(0);
            }
            mises_en_page.push(mettre_en_page(ui.painter(), t, m, dispo));
            &mises_en_page[mises_en_page.len() - 1]
        }
    };
    let total = mp.largeurs.iter().sum::<f32>().max(dispo);
    let h_corps: f32 = mp.lignes.iter().chain(&mp.pieds).map(|r| r.hauteur).sum();
    let palette = teintes(ui);
    // En clair, l'en-tête est dans son bandeau marine : un peu d'air au-dessus.
    if palette.entete_fond.is_some() && mp.h_entete > 0.0 {
        ui.add_space(4.0);
    }
    let entete = (mp.h_entete > 0.0).then(|| ui.allocate_exact_size(vec2(dispo, mp.h_entete), Sense::hover()).0);
    // Sans la marge de contenu de la fenêtre (10 points à droite) : le tableau a mesuré ses colonnes sur toute la
    // largeur, et la marge le faisait déborder d'autant — sa dernière colonne rognée et fondue, pour rien. En largeur
    // seulement : dans la conversation qui défile, egui ne donnait à une zone qui défile aussi en hauteur que la place
    // restée visible au-dessus d'elle — 64 points, trois lignes, au bas d'une longue conversation.
    let sortie = egui::ScrollArea::horizontal()
        .id_salt(id.with("tableau"))
        .content_margin(Margin::ZERO)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let (r, _) = ui.allocate_exact_size(vec2(total, h_corps.max(1.0)), Sense::hover());
            corps_du_tableau(ui, r, mp);
        });
    // Les barres de défilement d'egui ne se montrent qu'au survol : un fondu au bord dit qu'il reste des colonnes à
    // droite. `inner_rect` est la place offerte au départ : sa largeur est celle de la vue, mais pas sa hauteur (la
    // place restée visible) ; la vue a celle du tableau.
    let largeur_vue = sortie.inner_rect.width().min(sortie.content_size.x);
    let vue = Rect::from_min_size(sortie.inner_rect.min, vec2(largeur_vue, sortie.content_size.y));
    let decale = sortie.state.clamped_offset();
    let reste = sortie.content_size - vue.size() - decale;
    let p = ui.painter_at(vue);
    if reste.x > 1.0 {
        fondu(&p, Rect::from_min_max(pos2(vue.right() - 32.0, vue.top()), vue.right_bottom()), false, palette.fond);
    }
    if let Some(r) = entete {
        // Le bandeau marine de l'en-tête, en clair : 8 points de plus de chaque côté, 5 au-dessus.
        let bandeau = Rect::from_min_max(pos2(r.left() - 8.0, r.top() - 5.0), pos2(r.right() + 8.0, r.bottom()));
        let p = ui.painter_at(if palette.entete_fond.is_some() { bandeau } else { r });
        let fond_d_entete = match palette.entete_fond {
            Some(f) => {
                p.rect_filled(bandeau, 4.0, f);
                f
            }
            None => {
                p.hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, palette.filet_fort));
                palette.fond
            }
        };
        let origine = pos2(r.left() - decale.x, r.top());
        rangee_de_cellules(&p, origine, &mp.entete, &mp.largeurs, palette.entete_encre);
        if reste.x > 1.0 {
            fondu(
                &p,
                Rect::from_min_max(pos2(r.right() - 32.0, bandeau.top()), r.right_bottom()),
                false,
                fond_d_entete,
            );
        }
    }
}

/// Un fondu de `plein` (le fond de la carte, ou du bandeau d'en-tête) sur `r` : de transparent à opaque, vers la
/// droite ou vers le bas.
fn fondu(p: &egui::Painter, r: Rect, vers_le_bas: bool, plein: Color32) {
    let rien = Color32::TRANSPARENT;
    let (hd, bg) = if vers_le_bas { (rien, plein) } else { (plein, rien) };
    let mut m = egui::Mesh::default();
    m.colored_vertex(r.left_top(), rien);
    m.colored_vertex(r.right_top(), hd);
    m.colored_vertex(r.right_bottom(), plein);
    m.colored_vertex(r.left_bottom(), bg);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(egui::Shape::mesh(m));
}

/// Le corps d'un tableau et son pied, peints dans `r` : seulement les rangées à l'écran ; celle sous la souris
/// s'éclaire.
fn corps_du_tableau(ui: &egui::Ui, r: Rect, mp: &MiseEnPage) {
    let t = teintes(ui);
    let p = ui.painter();
    let ecran = ui.clip_rect();
    let mut y = r.top();
    for (i, l) in mp.lignes.iter().enumerate() {
        let zone = Rect::from_min_size(pos2(r.left(), y), vec2(r.width(), l.hauteur));
        if zone.intersects(ecran) {
            if ui.rect_contains_pointer(zone) {
                p.rect_filled(zone, 0.0, t.survol);
            }
            rangee_de_cellules(p, pos2(r.left(), y + MARGE_RANGEE), &l.cellules, &mp.largeurs, t.encre);
            if i + 1 < mp.lignes.len() {
                p.hline(zone.x_range(), zone.bottom() - 0.5, Stroke::new(1.0, t.filet));
            }
        }
        y += l.hauteur;
    }
    if !mp.pieds.is_empty() {
        p.hline(r.x_range(), y + 0.75, Stroke::new(1.5, t.filet_fort));
        for l in &mp.pieds {
            let haut = y + MARGE_RANGEE + 1.0;
            match l.cellules.first() {
                // Une note : sur toute la largeur, plus discrète qu'un total ; un montant seul, à droite.
                Some(c) if l.note => {
                    let x = if c.droite { r.right() - c.galley.size().x } else { r.left() };
                    p.galley(pos2(x, haut), c.galley.clone(), t.encre_2);
                }
                _ => rangee_de_cellules(p, pos2(r.left(), haut), &l.cellules, &mp.largeurs, t.encre),
            }
            y += l.hauteur;
        }
    }
}

/// Une rangée de cellules, de `origine` vers la droite : chacune dans sa colonne, à gauche ou à droite.
fn rangee_de_cellules(p: &egui::Painter, origine: Pos2, cellules: &[Cellule], largeurs: &[f32], encre: Color32) {
    let n = largeurs.len();
    let mut x = origine.x;
    for (k, c) in cellules.iter().enumerate() {
        let l = largeurs.get(k).copied().unwrap_or(0.0);
        let gx = if c.droite { x + l - marge(k, n) - c.galley.size().x } else { x };
        p.galley(pos2(gx, origine.y), c.galley.clone(), encre);
        x += l;
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    fn carte(json: &str) -> Carte {
        lire_carte(json).unwrap_or_else(|| panic!("la carte se lit : {json}"))
    }

    #[test]
    fn une_carte_se_copie_en_texte() {
        // Un tableau en colonnes pour Excel : une tabulation dans une cellule n'en fait pas deux ; le pied suit.
        let t = r#"{"type":"tableau","titre":"Factures","colonnes":["Client","Montant"],
                    "lignes":[["A\tB","1 200 $"],["C","300 $"]],"pied":["Total","1 500 $"]}"#;
        assert_eq!(en_texte(t).unwrap(), "Client\tMontant\nA B\t1 200 $\nC\t300 $\nTotal\t1 500 $");
        let a = r#"{"type":"agenda","titre":"Demain","jours":[{"jour":"lun. 26 oct.","evenements":[
                    {"heure":"9 h","titre":"Dentiste","detail":"Clinique Exemple"},"Collecte"]}]}"#;
        assert_eq!(en_texte(a).unwrap(), "Demain\n\nlun. 26 oct.\n9 h · Dentiste — Clinique Exemple\nCollecte");
        let b = r#"{"type":"brouillon","a":["x@exemple.ca","y@exemple.ca"],"objet":"Soumission","extrait":"Bonjour"}"#;
        assert_eq!(
            en_texte(b).unwrap(),
            "Brouillon prêt dans Outlook\nÀ : x@exemple.ca, y@exemple.ca\nObjet : Soumission\n\nBonjour"
        );
        assert_eq!(
            en_texte(r#"{"type":"apercu","chemin":"C:\\Exemple\\plan.pdf"}"#).unwrap(),
            "plan.pdf : C:\\Exemple\\plan.pdf"
        );
        assert_eq!(en_texte("{pas du json"), None);
        assert_eq!(titre_du_dessin("<svg><title>Plan &amp; coupe</title></svg>"), "Plan & coupe");
    }

    #[test]
    fn les_cartes_se_lisent_comme_dans_la_page() {
        // L'agenda du § 2.6 ; un jour qui n'est qu'une chaîne, un événement qui n'est qu'un titre, une sorte inconnue.
        let Carte::Agenda(a) = carte(
            r#"{"type":"agenda","titre":"Demain, jeudi 22 octobre","jours":[{"jour":"jeu. 22 oct.","evenements":[
               {"heure":"7 h 30","titre":"Démolition des jupettes","detail":"Chalet Exemple","sorte":"confirme"},
               {"heure":"16 h","titre":"Remettre la soumission","sorte":"echeance"}, "Appeler Julie",
               {"titre":"Visite","sorte":"inconnue"}]}, "ven. 23 oct."]}"#,
        ) else {
            panic!("un agenda")
        };
        assert_eq!(a.titre, "Demain, jeudi 22 octobre");
        assert_eq!(a.jours.len(), 2);
        let e = &a.jours[0].evenements;
        assert_eq!((e[0].heure.as_str(), e[0].sorte), ("7 h 30", Some(SorteEv::Confirme)));
        assert_eq!((e[1].detail.as_str(), e[1].sorte), ("", Some(SorteEv::Echeance)));
        assert_eq!((e[2].titre.as_str(), e[2].sorte), ("Appeler Julie", None));
        assert_eq!(e[3].sorte, None, "une sorte inconnue n'a pas de pastille");
        assert_eq!((a.jours[1].jour.as_str(), a.jours[1].evenements.len()), ("ven. 23 oct.", 0));
        // Sans jours, les événements seuls font un jour sans titre ; sans rien, « Rien au calendrier. ».
        let Carte::Agenda(a) = carte(r#"{"type":"agenda","evenements":["Réunion"]}"#) else { panic!() };
        assert_eq!((a.titre.as_str(), a.jours.len(), a.jours[0].jour.as_str()), ("Agenda", 1, ""));
        let Carte::Agenda(a) = carte(r#"{"type":"agenda"}"#) else { panic!() };
        assert!(a.jours.is_empty());

        // Le tableau du § 2.6 ; puis une ligne qui n'est qu'une valeur, des valeurs qui ne sont pas du texte, deux
        // pieds.
        let Carte::Tableau(t) = carte(
            r#"{"type":"tableau","titre":"Factures à vérifier","colonnes":["Fournisseur","Nº","Total"],
               "lignes":[["Matériaux Exemple","9013623395","31,08 $"],["Plomberie Exemple","96048","1 204,50 $"]],
               "pied":["Total","","1 235,58 $"]}"#,
        ) else {
            panic!()
        };
        assert_eq!((t.n, t.lignes.len(), t.pieds.len()), (3, 2, 1));
        assert_eq!(t.a_droite, [false, true, true]);
        assert!(t.droite(1, "Nº"), "l'en-tête d'une colonne de nombres s'aligne avec elle");
        let Carte::Tableau(t) = carte(
            r#"{"type":"tableau","lignes":["seule",[1,true,null,{"a":1}]],"pied":[["Sous-total","5"],["Total","6"]]}"#,
        ) else {
            panic!()
        };
        assert_eq!(t.titre, "Tableau");
        assert_eq!(t.lignes[0], ["seule"]);
        assert_eq!(t.lignes[1], ["1", "true", "", r#"{"a":1}"#]);
        assert_eq!((t.n, t.pieds.len()), (4, 2));
        let Carte::Tableau(t) = carte(r#"{"type":"tableau","titre":"Rien"}"#) else { panic!() };
        assert_eq!(t.n, 0, "« Tableau vide. »");

        // Le brouillon : à plusieurs, joints par « , ».
        let Carte::Brouillon(b) =
            carte(r#"{"type":"brouillon","a":["Julie","Marc"],"objet":"020 — Plan","extrait":"Bonjour","id":"00AB"}"#)
        else {
            panic!()
        };
        assert_eq!(
            (b.titre.as_str(), b.a.as_str(), b.id.as_str()),
            ("Brouillon prêt dans Outlook", "Julie, Marc", "00AB")
        );

        // L'aperçu : le titre d'office est le nom du fichier ; la sorte vient de l'extension.
        let Carte::Apercu(c) = carte(r#"{"type":"apercu","chemin":"C:/Exemple/Plan du sous-sol.SVG"}"#) else {
            panic!()
        };
        assert_eq!(
            (c.titre.as_str(), c.nom.as_str(), c.sorte),
            ("Plan du sous-sol.SVG", "Plan du sous-sol.SVG", Some(SorteFichier::Svg))
        );
        let Carte::Apercu(c) = carte(r#"{"type":"apercu","titre":"Rapport","chemin":"C:\\x\\rapport.docx"}"#) else {
            panic!()
        };
        assert_eq!((c.titre.as_str(), c.sorte), ("Rapport", None));
        let Carte::Apercu(c) = carte(r#"{"type":"apercu"}"#) else { panic!() };
        assert_eq!(c.titre, "Aperçu");

        // Illisibles : un JSON faux, pas encore fini, une sorte inconnue, ce qui n'est pas un objet.
        for faux in [r#"{"type":"tableau","lig"#, r#"{"type":"graphique"}"#, "[1,2]", "", r#"{"titre":"sans sorte"}"#] {
            assert_eq!(lire_carte(faux), None, "{faux}");
        }
        // Un JSON presque juste se lit quand même (une virgule de trop, des guillemets simples).
        assert!(matches!(lire_carte("{'type':'tableau','lignes':[['a','1'],],}"), Some(Carte::Tableau(_))));
    }

    #[test]
    fn les_nombres_s_alignent_a_droite() {
        for oui in [
            "937,05 $", "(45,00 $)", "-12 %", "8 h", "2026", "1 204,50 $", "1\u{a0}204,50\u{a0}$", "1\u{202f}204",
            "12 km", "3,5 m²", "45 PI2", "17 Ko", "$ 45", "− 3", "+2", "1'000", "9013623395", " 42 ", "30 min",
            "2,5 j", "182\u{a0}k$", "1,2 M$", "3 G$",
        ] {
            assert!(ressemble_a_un_nombre(oui), "{oui}");
        }
        for non in ["", "Total", "abc 12", "12 abc", "1.5 L", "10 h 30", "Nº", "(", "$", "-", "45 $ net"] {
            assert!(!ressemble_a_un_nombre(non), "{non}");
        }
        let l = |r: &[&[&str]]| -> Vec<Vec<String>> {
            r.iter().map(|x| x.iter().map(|s| s.to_string()).collect()).collect()
        };
        // La moitié des cellules pleines suffit ; les cellules vides ne comptent pas.
        let lignes = l(&[&["a", "1", "x"], &["b", "", "2"], &["c", "3", "y"], &["d", "4", "z"]]);
        assert_eq!(colonnes_a_droite(&lignes, &[], 3), [false, true, false]);
        let lignes = l(&[&["a", "1", "x"], &["b", "", "2"], &["c", "3", "5"]]);
        assert_eq!(colonnes_a_droite(&lignes, &[], 4), [false, true, true, false]);
        // Sans lignes, le pied décide.
        assert_eq!(colonnes_a_droite(&[], &l(&[&["Total", "5 $"]]), 2), [false, true]);
    }

    #[test]
    fn une_note_au_pied_prend_toute_la_largeur() {
        let Carte::Tableau(t) = carte(
            r#"{"type":"tableau","titre":"Météo","colonnes":["Jour","Min / max","Conditions"],
               "lignes":[["Samedi 24","0 / 11","Ensoleillé"],["Dimanche 25","2 / 14","Soleil"]],
               "pied":["Source : wttr.in, consultée le samedi 24 octobre 2026. Au-delà du lundi 26, aucune prévision."]}"#,
        ) else {
            panic!()
        };
        assert!(est_une_note(&t.pieds[0], t.n) && !est_une_note(&["Total".into(), "5".into()], 2));
        assert!(!est_une_note(&["seule".into()], 1), "un tableau d'une colonne n'a pas de note");
        let ctx = egui::Context::default();
        natif::polices(&ctx);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let p = ui.painter();
            let m = mesurer(p, &t, 1.0);
            // La note n'élargit pas la première colonne : « Dimanche 25 » la règle.
            let jour = cellule(p, "Dimanche 25", police_corps(), Color32::PLACEHOLDER, 0.0, f32::INFINITY).size().x;
            assert_eq!(m.max[0], (jour + MARGE_COLONNE).ceil(), "{:?}", m.max);
            let mp = mettre_en_page(p, &t, &m, 400.0);
            let note = &mp.pieds[0];
            assert!(note.note && note.cellules.len() == 1);
            // Sur la largeur de la carte : deux lignes, quand la première colonne seule lui en donnait cinq.
            let g = &note.cellules[0].galley;
            assert!(
                g.rows.len() <= 2 && g.size().x > 2.0 * mp.largeurs[0],
                "{} lignes de {}",
                g.rows.len(),
                g.size().x
            );
        });
    }

    #[test]
    fn un_dessin_se_complete_et_se_nomme() {
        // Sans l'espace de noms du SVG, comme dans une page web, il s'ajoute ; celui de xlink, si le dessin s'en sert.
        assert_eq!(
            svg_complet(r#"<svg viewBox="0 0 4 2"></svg>"#),
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 2"></svg>"#
        );
        assert!(matches!(svg_complet(r#"<svg xmlns="http://www.w3.org/2000/svg"><rect/></svg>"#), Cow::Borrowed(_)));
        let xlink = svg_complet(r##"<svg xmlns="http://www.w3.org/2000/svg"><use xlink:href="#a"/></svg>"##);
        assert!(xlink.starts_with(r#"<svg xmlns:xlink="http://www.w3.org/1999/xlink" xmlns="#), "{xlink}");
        assert_eq!(svg_complet("pas un dessin"), "pas un dessin");
        // Son titre : son premier <title>, ses entités lues, ses blancs resserrés.
        assert_eq!(
            titre_du_dessin("<svg><title>Plan du  sous-sol &amp; garage</title></svg>"),
            "Plan du sous-sol & garage"
        );
        assert_eq!(titre_du_dessin("<svg><rect/></svg>"), "Dessin");
        assert_eq!(titre_du_dessin("<svg><title> </title></svg>"), "Dessin");
        // Le nom de ses fichiers.
        assert_eq!(nom_de_fichier("Plan : sous-sol / garage ?"), "Plan sous-sol garage");
        assert_eq!(nom_de_fichier("CON"), "Dessin");
        assert_eq!(nom_de_fichier("..."), "Dessin");
        assert_eq!(nom_de_fichier(&"é".repeat(80)).chars().count(), 60);
        assert_eq!(avec_extension(PathBuf::from(r"C:\x\plan"), 1), PathBuf::from(r"C:\x\plan.png"));
        assert_eq!(avec_extension(PathBuf::from(r"C:\x\plan"), 2), PathBuf::from(r"C:\x\plan.svg"));
        assert_eq!(avec_extension(PathBuf::from(r"C:\x\plan.SVG"), 1), PathBuf::from(r"C:\x\plan.SVG"));
        assert_eq!(avec_extension(PathBuf::from(r"C:\x\plan.v2"), 1), PathBuf::from(r"C:\x\plan.v2.png"));
        // Sa taille à l'écran : toute la largeur, agrandi s'il le faut, la hauteur au plus ; son rendu, au palier.
        assert_eq!(remplir(vec2(800.0, 500.0), vec2(400.0, 1000.0)), vec2(400.0, 250.0));
        assert_eq!(remplir(vec2(100.0, 50.0), vec2(400.0, 1000.0)), vec2(400.0, 200.0), "agrandi");
        assert_eq!(remplir(vec2(500.0, 1000.0), vec2(400.0, 400.0)), vec2(200.0, 400.0), "la hauteur au plus");
        assert_eq!((palier(1.0, 2048), palier(700.4, 2048), palier(9000.0, 2048)), (256, 768, 2048));
    }

    #[test]
    fn un_dessin_s_ouvre_et_s_enregistre() {
        // « Ouvrir en grand » : le dessin dans dessins\ du dossier local, à son nom ; le même dessin, le même fichier.
        let local = std::env::temp_dir().join(format!("eve-dessins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&local);
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4 2"><rect width="4" height="2"/></svg>"#;
        let chemin = dessin_dans_un_fichier(&local, "Plan : sous-sol", 0xABCD_EF12, svg).unwrap();
        assert_eq!(chemin, local.join("dessins").join("Plan sous-sol cdef12.svg"));
        assert_eq!(std::fs::read_to_string(&chemin).unwrap(), svg);
        assert_eq!(dessin_dans_un_fichier(&local, "Plan : sous-sol", 0xABCD_EF12, svg).unwrap(), chemin);
        // « Enregistrer » : en PNG, ou en SVG, selon le nom choisi.
        ecrire_le_dessin(&local.join("plan.png"), svg).unwrap();
        assert!(std::fs::read(local.join("plan.png")).unwrap().starts_with(b"\x89PNG"));
        ecrire_le_dessin(&local.join("plan.SVG"), svg).unwrap();
        assert_eq!(std::fs::read_to_string(local.join("plan.SVG")).unwrap(), svg);
        assert!(ecrire_le_dessin(&local.join("faux.png"), "<svg").is_err());
        let _ = std::fs::remove_dir_all(&local);
    }

    #[test]
    fn un_dessin_se_rend_sur_une_feuille_blanche() {
        // Sans espace de noms, un carré rouge et du texte sans police : il se rend à la largeur demandée, sur du blanc.
        let svg = svg_complet(concat!(
            r#"<svg viewBox="0 0 200 100"><rect x="0" y="0" width="50" height="50" fill="red"/>"#,
            r#"<text x="100" y="80" font-size="30">Salon</text></svg>"#
        ))
        .into_owned();
        let (image, intrinseque) = dessin_en_pixels(&svg, 512, 2048).expect("le dessin se rend");
        assert_eq!((image.size, intrinseque), ([512, 256], vec2(200.0, 100.0)));
        assert_eq!(image.pixels[0], Color32::RED, "le carré, en haut à gauche");
        assert_eq!(image.pixels[511], Color32::WHITE, "la feuille, en haut à droite");
        // Son texte se dessine, dans la police d'office (Segoe UI, sur ce poste).
        #[cfg(windows)]
        {
            let texte = (140..210).flat_map(|y| (256..480).map(move |x| y * 512 + x));
            assert!(texte.filter(|&i| image.pixels[i] != Color32::WHITE).count() > 300, "le texte se dessine");
        }
        // Plus grand que la texture : son grand côté s'y tient.
        assert_eq!(dessin_en_pixels(&svg, 8192, 2048).unwrap().0.size, [2048, 1024]);
        // Un SVG faux ne se rend pas, sans paniquer.
        assert!(matches!(dessin_en_pixels("<svg><rect", 512, 2048), Err(Probleme::DessinIllisible)));
        // En PNG : 2048 pixels de grand côté.
        let png = png_du_dessin(&svg).unwrap();
        let lecteur = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
        assert_eq!((lecteur.info().width, lecteur.info().height), (2048, 1024));
    }

    #[test]
    fn les_colonnes_se_partagent_la_largeur() {
        // Tout tient : la place qui reste se partage selon la largeur de chacune.
        assert_eq!(largeurs(&[10.0, 10.0], &[100.0, 50.0], 300.0), [200.0, 100.0]);
        // Trop étroit pour une ligne : chacune se resserre d'autant plus qu'elle le peut.
        assert_eq!(largeurs(&[20.0, 40.0], &[100.0, 60.0], 110.0), [60.0, 50.0]);
        // Plus étroit que le plus étroit : le tableau défile.
        assert_eq!(largeurs(&[80.0, 90.0], &[100.0, 200.0], 120.0), [80.0, 90.0]);
        assert_eq!(largeurs(&[0.0, 0.0], &[0.0, 0.0], 100.0), [50.0, 50.0]);
        assert!(largeurs(&[], &[], 100.0).is_empty());
    }

    #[test]
    fn un_apercu_ne_s_ouvre_que_sous_les_dossiers_permis() {
        let racines = [
            normaliser(r"C:\Exemple\Maison").unwrap(),
            normaliser(r"C:\Users\Sam\AppData\Local\Eve\specialistes\famille").unwrap(),
        ];
        let permis = |c: &str| juger(c, &racines);
        // La même chose, écrite autrement : les barres, la casse, « . », « .. » qui reste dedans, le préfixe étendu.
        for meme in [
            r"C:\Exemple\Maison\plan.svg",
            "C:/Exemple/Maison/plan.svg",
            r"c:\exemple\MAISON\plan.svg",
            r"C:\Exemple\Maison\.\sous\..\plan.svg",
            r"\\?\C:\Exemple\Maison\plan.svg",
            r"\??\C:\Exemple\Maison\plan.svg",
            r"\\.\C:\Exemple\Maison\plan.svg",
            r"C:\\Exemple\\Maison\\plan.svg",
            "  C:\\Exemple\\Maison\\plan.svg  ",
        ] {
            let c = permis(meme).unwrap_or_else(|r| panic!("{meme} : {r:?}"));
            let attendu = ["exemple", "maison", "plan.svg"];
            assert!(c.disque == 'C' && c.composants.len() == 3, "{meme}");
            assert!(attendu.iter().zip(&c.composants).all(|(a, b)| meme_nom(a, b)), "{meme}");
        }
        assert_eq!(
            permis(r"C:\Exemple\Maison\plan.svg").unwrap().chemin(),
            PathBuf::from(r"C:\Exemple\Maison\plan.svg")
        );
        assert!(permis(r"C:\Users\Sam\AppData\Local\Eve\specialistes\famille\pieces\2026-10-23\a.png").is_ok());
        // Un « .. » qui sort, un dossier voisin au nom qui commence pareil, un autre disque : dehors.
        for dehors in [
            r"C:\Exemple\Maison\..\Comptabilité\bilan.pdf",
            r"C:\Exemple\Maison\..\..\Windows\win.ini.png",
            r"C:\Exemple\Maisons\plan.svg",
            r"C:\Exemple\Maison.png",
            r"D:\Exemple\Maison\plan.svg",
            r"\\?\D:\Exemple\Maison\plan.svg",
            r"C:\Exemple",
        ] {
            assert_eq!(permis(dehors), Err(Refus::Dehors), "{dehors}");
        }
        // Le réseau, refusé avant toute ouverture, sous toutes ses formes.
        for reseau in [
            r"\\serveur\partage\plan.svg",
            "//serveur/partage/plan.svg",
            r"\\?\UNC\serveur\partage\plan.svg",
            r"\\?\unc\serveur\partage\plan.svg",
            r"\??\UNC\serveur\partage\plan.svg",
            r"\\.\UNC\serveur\partage\plan.svg",
            r"\\127.0.0.1\c$\Exemple\Maison\plan.svg",
        ] {
            assert_eq!(permis(reseau), Err(Refus::Reseau), "{reseau}");
        }
        // Ce qui n'est pas le chemin complet d'un fichier du poste.
        for incomplet in [
            "plan.svg",
            r"\Exemple\Maison\plan.svg",
            r"C:Exemple\Maison\plan.svg",
            r"\\.\pipe\eve",
            r"\\?\Volume{0b1c2d3e-0000-0000-0000-000000000000}\plan.svg",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\plan.svg",
            r"C:\Exemple\Maison\CON.png",
            r"C:\Exemple\Maison\nul",
            r"C:\Exemple\Maison\com1.svg",
            r"C:\Exemple\Maison\LPT¹.png",
            r"C:\Exemple\Maison\plan.svg:flux",
            r"C:\Exemple\Maison\pl*n.svg",
            r"C:\Exemple\Maison\plan?.svg",
            "C:\\Exemple\\Maison\\plan\u{0}.svg",
            "1:\\Exemple\\Maison\\plan.svg",
            "",
            "   ",
        ] {
            assert_eq!(permis(incomplet), Err(Refus::Incomplet), "{incomplet:?}");
        }
        // Les noms réservés ne le sont qu'entiers : « CONSOLE.png », « com10 » sont des noms ordinaires.
        assert!(permis(r"C:\Exemple\Maison\CONSOLE.png").is_ok());
        assert!(permis(r"C:\Exemple\Maison\com10.png").is_ok());
        // La casse comme Windows : « É » vaut « é », « ß » ne vaut pas « SS ».
        let racines = [normaliser(r"C:\Exemple\Comptabilité").unwrap(), normaliser(r"C:\Straße").unwrap()];
        assert!(juger(r"C:\EXEMPLE\COMPTABILITÉ\bilan.pdf", &racines).is_ok());
        assert_eq!(juger(r"C:\STRASSE\plan.png", &racines), Err(Refus::Dehors));
        // Une racine sur le réseau n'ouvre rien.
        let c = Cartes::nouvelles(Some(PathBuf::from(r"\\nas\famille")), PathBuf::from(r"C:\Eve\local"));
        assert_eq!(c.racines, [normaliser(r"C:\Eve\local").unwrap()]);
    }

    #[test]
    fn la_vignette_d_une_page_html() {
        let html = "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Plan &amp; devis — l&#39;atelier</title>\
                    <style>body { color: red }</style><script>var x = '<b>non</b>';</script></head>\
                    <body><!-- caché --><h1>Le&nbsp;plan</h1>\n<p>Deux   chambres,&eacute;tage&hellip;</p>\
                    <svg><text>étiquette</text></svg><scripts>garde</scripts>&inconnue; &#x41;</body></html>";
        let (titre, extrait) = vignette_html(html);
        assert_eq!(titre, "Plan & devis — l'atelier");
        assert_eq!(extrait, "Le plan Deux chambres,étage… garde &inconnue; A");
        assert_eq!(vignette_html("du texte seul"), (String::new(), "du texte seul".into()));
        assert_eq!(vignette_html("<p class=\"pas fini"), (String::new(), String::new()), "une balise jamais fermée");
        assert_eq!(
            vignette_html("<TITLE>Majuscules</TITLE><BODY>Bonjour</BODY>"),
            ("Majuscules".into(), "Bonjour".into())
        );
    }

    #[test]
    fn les_petits_outils() {
        assert_eq!(taille_lisible(10), "1 Ko");
        assert_eq!(taille_lisible(17 * 1024), "17 Ko");
        assert_eq!(taille_lisible(2_411_725), "2,3 Mo");
        assert_eq!(taille_lisible(APERCU_MAX + 1), "25,0 Mo");
        assert_eq!(
            en_lignes("Bonjour   Julie,\r\n\tLe plan  est approuvé.\n\nSam"),
            "Bonjour Julie,\nLe plan est approuvé.\n\nSam"
        );
        assert_eq!((nombre_de(1, "page"), nombre_de(12, "ligne")), ("1 page".into(), "12 lignes".into()));
        assert_eq!(texte(Some(&serde_json::json!(2.5))), "2.5");
        assert_eq!(texte(Some(&serde_json::json!(["a", 1]))), r#"["a",1]"#);
        assert_eq!(sorte_de_fichier("photo.JPEG"), Some(SorteFichier::Image));
        assert_eq!(sorte_de_fichier("page.htm"), Some(SorteFichier::Page));
        assert_eq!(sorte_de_fichier("plan.svg.bak"), None);
        assert_eq!(sorte_de_fichier("sans extension"), None);
        assert_eq!(ajuster(vec2(1000.0, 500.0), vec2(400.0, 360.0)), vec2(400.0, 200.0));
        assert_eq!(ajuster(vec2(100.0, 50.0), vec2(400.0, 360.0)), vec2(100.0, 50.0), "jamais agrandie");
        assert_eq!(ajuster(vec2(816.0, 1056.0), vec2(700.0, 360.0)).y, 360.0);
        assert!(nom_reserve("Aux.txt") && nom_reserve("CONOUT$") && !nom_reserve("auxiliaire"));
    }

    /// Les lectures du fil, sur de vrais fichiers : une image, un SVG, une page, un PDF (Windows les dessine), un
    /// fichier trop gros ; le second jugement, sur le disque ; un fichier inchangé ne se relit pas.
    #[test]
    fn le_fil_lit_les_apercus_permis() {
        let base = std::env::temp_dir().join(format!("eve-cartes-{}", std::process::id()));
        let (travail, local, dehors) = (base.join("travail"), base.join("local"), base.join("dehors"));
        for d in [&travail, &local, &dehors] {
            std::fs::create_dir_all(d).unwrap();
        }
        // Une image PNG de 4 × 2, écrite par la crate png.
        let mut png_octets = Vec::new();
        {
            let mut e = png::Encoder::new(&mut png_octets, 4, 2);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(&[200u8; 4 * 2 * 4]).unwrap();
        }
        std::fs::write(travail.join("image.png"), &png_octets).unwrap();
        std::fs::write(dehors.join("image.png"), &png_octets).unwrap();
        // Un SVG rouge, qui voudrait lire un fichier du poste : resvg ne le lit pas.
        let svg = concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20">"#,
            r#"<rect width="40" height="20" fill="red"/>"#,
            r#"<image href="C:\Windows\win.ini" width="5" height="5"/></svg>"#
        );
        std::fs::write(travail.join("plan.svg"), svg).unwrap();
        std::fs::write(local.join("page.html"), "<title>Essai</title><p>Bonjour</p>").unwrap();
        std::fs::write(travail.join("soumission.pdf"), pdf_d_essai()).unwrap();
        std::fs::File::create(travail.join("gros.png")).unwrap().set_len(APERCU_MAX + 1).unwrap();
        let racines = vec![travail.clone(), local.clone()];
        let demande = |chemin: PathBuf, sorte, connu| Demande {
            cle: String::new(),
            chemin,
            sorte,
            connu,
            limite: 2048,
            ctx: egui::Context::default(),
        };
        #[cfg(windows)]
        systeme::initialiser();
        let lu = |d: Demande| lire(&d, &racines);

        #[cfg(windows)]
        {
            let Lu::Image { image, intrinseque, empreinte, .. } =
                lu(demande(travail.join("image.png"), SorteFichier::Image, None))
            else {
                panic!("l'image se lit")
            };
            assert_eq!((image.size, intrinseque), ([4, 2], vec2(4.0, 2.0)));
            assert!(matches!(
                lu(demande(travail.join("image.png"), SorteFichier::Image, Some(empreinte))),
                Lu::Inchange
            ));
            let Lu::Image { image, intrinseque, pages, .. } =
                lu(demande(travail.join("soumission.pdf"), SorteFichier::Pdf, None))
            else {
                panic!("le PDF se lit")
            };
            assert_eq!(pages, 2);
            assert!((intrinseque.y / intrinseque.x - 792.0 / 612.0).abs() < 0.01, "une page Lettre : {intrinseque:?}");
            assert!(image.size[1] > 100 && image.pixels[0] == Color32::WHITE, "la page est blanche dans son coin");
        }
        let Lu::Image { image, intrinseque, .. } = lu(demande(travail.join("plan.svg"), SorteFichier::Svg, None))
        else {
            panic!("le SVG se lit")
        };
        assert_eq!((image.size, intrinseque), ([80, 40], vec2(40.0, 20.0)), "à deux fois sa taille");
        assert_eq!(image.pixels[0], Color32::RED);
        let Lu::Page { titre, extrait, .. } = lu(demande(local.join("page.html"), SorteFichier::Page, None)) else {
            panic!("la page se lit")
        };
        assert_eq!((titre.as_str(), extrait.as_str()), ("Essai", "Bonjour"));
        let echec = |l: Lu| match l {
            Lu::Echec { probleme, existe, .. } => Some((probleme, existe)),
            _ => None,
        };
        assert_eq!(
            echec(lu(demande(travail.join("gros.png"), SorteFichier::Image, None))),
            Some((Probleme::TropGros(APERCU_MAX + 1), true))
        );
        assert_eq!(
            echec(lu(demande(travail.join("absent.png"), SorteFichier::Image, None))),
            Some((Probleme::Introuvable, false))
        );
        // Le second jugement : même si la forme passait, ce que le chemin désigne vraiment est dehors.
        assert_eq!(
            echec(lu(demande(dehors.join("image.png"), SorteFichier::Image, None))),
            Some((Probleme::Refuse(Refus::Dehors), false))
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Un PDF de deux pages au format Lettre, écrit à la main, ses positions comptées.
    fn pdf_d_essai() -> Vec<u8> {
        let dessin = "0 0 1 rg 100 600 200 100 re f";
        let objets = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{dessin}\nendstream", dessin.len()),
        ];
        let mut o = b"%PDF-1.4\n".to_vec();
        let mut positions = Vec::new();
        for (i, x) in objets.iter().enumerate() {
            positions.push(o.len());
            o.extend(format!("{} 0 obj\n{x}\nendobj\n", i + 1).as_bytes());
        }
        let xref = o.len();
        o.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objets.len() + 1).as_bytes());
        for p in positions {
            o.extend(format!("{p:010} 00000 n \n").as_bytes());
        }
        o.extend(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objets.len() + 1).as_bytes(),
        );
        o
    }
}
