//! L'historique des conversations, dans la fenêtre native (le lot 6.3) : le panneau « Historique » de la page
//! (`page\eve.html` : sa section `#historique`, ses styles `.hist-*` et son module `Historique` ; l'inventaire :
//! `docs\fenetre-native.md`, § 2.11, H1 à H7), que la fenêtre pose par-dessus la conversation. La liste des
//! conversations gardées, groupées par jour, et la recherche ; une conversation relue en bulles — celles du fil
//! (`natif::bulle`), ses cartes comprises — ; « Reprendre cette conversation ».
//!
//! Le panneau ne parle pas au chef : il rend une [`Sortie`] (fermer, reprendre une conversation, le geste d'une carte),
//! que la fenêtre transmet. Il ne lit jamais le disque dans l'image : un fil lit l'historique (`historique.rs`, la
//! lecture même de la route `/historique` de la page) et lui rend ce qu'il a lu. Une réponse dépassée par une demande
//! plus récente est jetée, et une recherche dépassée avant d'être lue ne se lit même pas.
//!
//! La fenêtre le pose dans une [`egui::Area`] au premier plan, sur la place qu'elle lui donne (il la prend toute) ;
//! Échap passe par [`Historique::echap`], et un clic hors du panneau le ferme. `eve --essai-historique` le pose ainsi.

use std::panic;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
use eframe::egui::{
    self, Align2, Color32, FontFamily, FontId, Galley, Id, Margin, Rect, RichText, Sense, Shadow, Stroke, StrokeKind,
    UiBuilder, pos2, vec2,
};

use crate::cartes::{Cartes, Geste};
use crate::historique::{self, Entree, Resume};
use crate::natif::{self, Ligne, couleurs, melange};

/// Les conversations de la liste : 300 au plus, comme la route `/historique` de la page.
const LIMITE: usize = 300;
/// La recherche se relance ce temps après la dernière frappe (la page : 220 ms).
const RELANCE: f64 = 0.22;
/// Le panneau entre en fondu, en montant de 8 points (la page : 0,18 s).
const ENTREE: f64 = 0.18;
/// Une première lecture plus longue montre une roue : sur un disque lent, le panneau ne reste pas vide sans rien dire.
const ROUE: f64 = 0.3;
/// La tête du panneau : 9 points au-dessus de ses boutons de 32, 8 au-dessous ; son filet en plus.
const TETE: f32 = 49.0;
/// Le pied d'une conversation, « Reprendre cette conversation » toujours en vue : son filet, 10 points, le bouton de
/// 36, 12 points.
const PIED: f32 = 59.0;
/// La marge du corps, de chaque côté (la page : `.panneau-corps`, 14 px).
const MARGE: f32 = 14.0;
/// La barre de défilement flotte dans la marge de droite : la zone qui défile y avance d'autant, son contenu non
/// (`content_margin` du style de la fenêtre).
const BARRE: f32 = 10.0;

/// Les messages de la page (H4), tels quels.
const SANS_HISTORIQUE: &str = "Aucune conversation gardée pour l'instant : elles s'ajoutent ici au fil des échanges.";
const ILLISIBLE: &str = "L'historique ne se lit pas pour l'instant.";
const CONVERSATION_ILLISIBLE: &str = "Cette conversation ne se lit pas pour l'instant.";
const CONVERSATION_VIDE: &str = "Cette conversation est vide.";

/// Les jours, du dimanche au samedi, et les mois, en entier et abrégés comme les écrit le français du Canada (la page
/// les tenait du navigateur, `toLocaleDateString("fr-CA")`).
const JOURS: [&str; 7] = ["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"];
const MOIS: [&str; 12] = [
    "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre",
    "décembre",
];
const MOIS_COURTS: [&str; 12] =
    ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juill.", "août", "sept.", "oct.", "nov.", "déc."];

/// Les icônes du panneau (Segoe Fluent Icons) ; `HISTORIQUE` est celle de son bouton, dans la barre de la fenêtre.
pub mod icones {
    pub const HISTORIQUE: char = '\u{E81C}';
    pub const RETOUR: char = '\u{E76B}';
    pub const CHERCHER: char = '\u{E721}';
}

/// Ce que le panneau demande à la fenêtre.
#[derive(Debug, Clone, PartialEq)]
pub enum Sortie {
    /// × : le panneau se ferme.
    Fermer,
    /// « Reprendre cette conversation » (son id) : la fenêtre envoie `reprendre` au chef, puis ferme le panneau.
    Reprendre(String),
    /// Le geste d'une carte d'une conversation relue (ouvrir un fichier, un brouillon d'Outlook) : la fenêtre le rend
    /// au chef, comme ceux des cartes du fil.
    Geste(Geste),
}

/// Les éléments du panneau qui ont un id fixe : `Context::read_response` rend ainsi leur place (l'essai les vise).
#[derive(Clone, Copy)]
pub(crate) enum Element<'a> {
    Cherche,
    Effacer,
    Retour,
    Fermer,
    Reprendre,
    Liste,
    Echanges,
    /// Une conversation de la liste, par son id.
    Conversation(&'a str),
}

pub(crate) fn id_de(element: Element<'_>) -> Id {
    let base = Id::unique("historique-des-conversations");
    match element {
        Element::Cherche => base.with("cherche"),
        Element::Effacer => base.with("effacer"),
        Element::Retour => base.with("retour"),
        Element::Fermer => base.with("fermer"),
        Element::Reprendre => base.with("reprendre"),
        Element::Liste => base.with("liste"),
        Element::Echanges => base.with("echanges"),
        Element::Conversation(id) => base.with(("conversation", id)),
    }
}

/// La liste reçue, et la recherche qu'elle sert ; `conversations` vaut `None` si la lecture a échoué.
struct Liste {
    recherche: String,
    conversations: Option<Vec<Resume>>,
}

/// La conversation ouverte, que le panneau montre au lieu de la liste.
struct Ouverte {
    id: String,
    echanges: Vec<Entree>,
}

/// Le panneau de l'historique : ce qu'il montre, ce qu'il attend du fil qui lit.
pub struct Historique {
    /// Le dossier local d'où l'historique se lit ; `None` : celui d'Eve, que le fil demande lui-même.
    local: Option<PathBuf>,
    lecteur: Lecteur,
    /// La personne dont l'historique est à l'écran (il se range par personne) ; `None` avant la première image.
    qui: Option<String>,
    /// Ce qu'on cherche, tel qu'on l'écrit ; l'instant de la dernière frappe, tant que la recherche n'est pas relancée.
    recherche: String,
    frappe: Option<f64>,
    /// La dernière liste reçue ; depuis quand une liste est attendue.
    liste: Option<Liste>,
    attendue: Option<f64>,
    /// La conversation ouverte ; une conversation est demandée et pas encore lue.
    ouverte: Option<Ouverte>,
    conversation_attendue: bool,
    /// Ce que dit la liste d'une conversation qui ne s'ouvre pas (H4).
    avis: Option<&'static str>,
    /// Le numéro de la dernière demande de liste, et de conversation : une réponse d'un autre numéro est dépassée.
    demande_liste: u64,
    demande_conversation: u64,
    /// Une liste arrivée, une conversation ouverte : elles se montrent depuis leur début. egui garde le défilement
    /// d'une zone par son id, d'une conversation à l'autre et d'une ouverture du panneau à l'autre ; le retour d'une
    /// conversation à la liste, lui, la retrouve où on l'avait laissée.
    remonter_liste: bool,
    remonter_echanges: bool,
    /// La recherche prend le focus à la prochaine image (à l'ouverture, au retour à la liste).
    focaliser: bool,
    /// L'instant de la première image du panneau : il y entre en fondu.
    apparition: Option<f64>,
}

impl Historique {
    /// Le panneau, sur l'historique d'Eve (son dossier local) : la liste se lit dès la première image, et la recherche
    /// prend le focus.
    pub fn nouveau() -> Historique {
        Historique {
            local: None,
            lecteur: Lecteur::PasLance,
            qui: None,
            recherche: String::new(),
            frappe: None,
            liste: None,
            attendue: None,
            ouverte: None,
            conversation_attendue: false,
            avis: None,
            demande_liste: 0,
            demande_conversation: 0,
            remonter_liste: true,
            remonter_echanges: false,
            focaliser: true,
            apparition: None,
        }
    }

    /// Le même panneau, sur l'historique d'un autre dossier local : celui d'un essai, qui ne lit ainsi jamais celui
    /// d'une vraie Eve.
    pub(crate) fn sous(mut self, local: PathBuf) -> Historique {
        self.local = Some(local);
        self
    }

    /// Échap (H1, H6) : d'une conversation, revient à la liste ; vrai s'il y était déjà — la fenêtre ferme alors le
    /// panneau.
    pub fn echap(&mut self) -> bool {
        if self.ouverte.is_some() {
            self.retour();
            false
        } else {
            true
        }
    }

    /// Le panneau, dans la place que la fenêtre lui donne (`ui.max_rect()`, qu'il prend toute) ; `qui` : la personne
    /// devant le poste (`qui` de l'état publié). Rend ce que la personne y a demandé.
    pub fn dessiner(&mut self, ui: &mut egui::Ui, qui: &str, cartes: &mut Cartes) -> Option<Sortie> {
        let temps = ui.input(|i| i.time);
        if matches!(self.lecteur, Lecteur::PasLance) {
            self.lecteur = Lecteur::lancer(self.local.clone(), ui.ctx().clone());
        }
        self.relever();
        // La première image, ou une autre personne devant le poste : sa liste à elle.
        if self.qui.as_deref() != Some(qui) {
            self.qui = Some(qui.to_string());
            if self.ouverte.is_some() {
                self.retour();
            }
            (self.avis, self.frappe) = (None, None);
            self.demander_la_liste(temps);
        }
        if let Some(frappe) = self.frappe {
            let reste = frappe + RELANCE - temps;
            if reste <= 0.0 {
                self.frappe = None;
                self.demander_la_liste(temps);
            } else {
                ui.ctx().request_repaint_after(Duration::from_secs_f64(reste));
            }
        }
        // L'entrée : un fondu, en montant de 8 points.
        let debut = *self.apparition.get_or_insert(temps);
        let t = ((temps - debut) / ENTREE).clamp(0.0, 1.0) as f32;
        let entre = 1.0 - (1.0 - t) * (1.0 - t);
        if t < 1.0 {
            ui.ctx().request_repaint();
        }
        let place = ui.max_rect();
        ui.allocate_rect(place, Sense::hover());
        ui.multiply_opacity(entre);
        let cadre = place.translate(vec2(0.0, 8.0 * (1.0 - entre)));
        let p = ui.painter();
        // L'ombre du panneau (`--ombre-panneau`) : une proche et nette, une large et douce.
        for ombre in [
            Shadow { offset: [0, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(100) },
            Shadow { offset: [0, 16], blur: 44, spread: 0, color: Color32::from_black_alpha(150) },
        ] {
            p.add(ombre.as_shape(cadre, 14));
        }
        p.rect(cadre, 14.0, couleurs::SURFACE, Stroke::new(1.0, couleurs::FILET), StrokeKind::Inside);
        p.hline(cadre.x_range(), cadre.top() + TETE + 0.5, Stroke::new(1.0, couleurs::FILET));
        let tete = Rect::from_min_size(cadre.min, vec2(cadre.width(), TETE));
        let corps = Rect::from_min_max(pos2(cadre.left(), tete.bottom() + 1.0), cadre.max);
        let aujourdhui = crate::journal::date();
        let sortie = self.tete(ui, tete);
        let du_corps = if self.ouverte.is_some() {
            self.conversation(ui, corps, cartes, &aujourdhui)
        } else {
            self.liste(ui, corps, temps, &aujourdhui);
            None
        };
        sortie.or(du_corps)
    }

    /// ← (H6) : la liste, et la recherche reprend le focus.
    fn retour(&mut self) {
        self.ouverte = None;
        // Une conversation demandée qui arriverait après ne rouvre rien.
        self.demande_conversation += 1;
        self.conversation_attendue = false;
        self.focaliser = true;
    }

    /// Une demande au fil ; faux s'il ne la reçoit pas (il ne part pas, ou s'est arrêté).
    fn envoyer(&mut self, demande: Demande) -> bool {
        if let Lecteur::Lance { envoi, .. } = &self.lecteur
            && envoi.send(demande).is_ok()
        {
            return true;
        }
        self.lecteur = Lecteur::Arrete;
        false
    }

    /// La liste, pour la recherche telle qu'elle est écrite.
    fn demander_la_liste(&mut self, temps: f64) {
        self.demande_liste += 1;
        let (n, recherche) = (self.demande_liste, self.recherche.trim().to_string());
        let qui = self.qui.clone().unwrap_or_default();
        self.attendue.get_or_insert(temps);
        if !self.envoyer(Demande::Liste { n, qui, recherche: recherche.clone() }) {
            self.liste = Some(Liste { recherche, conversations: None });
            self.attendue = None;
        }
    }

    fn demander_la_conversation(&mut self, id: String) {
        self.demande_conversation += 1;
        let (n, qui) = (self.demande_conversation, self.qui.clone().unwrap_or_default());
        self.conversation_attendue = true;
        if !self.envoyer(Demande::Conversation { n, qui, id }) {
            self.conversation_attendue = false;
            self.avis = Some(CONVERSATION_ILLISIBLE);
        }
    }

    /// Ce que le fil a lu depuis la dernière image ; s'il s'est arrêté, ce qu'on attendait de lui ne viendra pas.
    fn relever(&mut self) {
        let Lecteur::Lance { recu, .. } = &self.lecteur else { return };
        let mut lues = Vec::new();
        let arrete = loop {
            match recu.try_recv() {
                Ok(r) => lues.push(r),
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        for r in lues {
            self.recevoir(r);
        }
        if arrete {
            self.lecteur = Lecteur::Arrete;
            if self.attendue.take().is_some() {
                self.liste = Some(Liste { recherche: self.recherche.trim().to_string(), conversations: None });
            }
            if std::mem::take(&mut self.conversation_attendue) {
                self.avis = Some(CONVERSATION_ILLISIBLE);
            }
        }
    }

    /// Une réponse du fil ; celle d'une demande dépassée est jetée.
    fn recevoir(&mut self, reponse: Reponse) {
        match reponse {
            Reponse::Liste { n, recherche, conversations } => {
                if n != self.demande_liste {
                    return;
                }
                self.liste = Some(Liste { recherche, conversations });
                (self.attendue, self.avis, self.remonter_liste) = (None, None, true);
            }
            Reponse::Conversation { n, id, echanges } => {
                if n != self.demande_conversation {
                    return;
                }
                self.conversation_attendue = false;
                match echanges {
                    None => self.avis = Some(CONVERSATION_ILLISIBLE),
                    Some(e) if e.is_empty() => self.avis = Some(CONVERSATION_VIDE),
                    Some(echanges) => {
                        self.ouverte = Some(Ouverte { id, echanges });
                        (self.avis, self.remonter_echanges) = (None, true);
                    }
                }
            }
        }
    }

    /// La tête (`.panneau-tete`) : le ruban, ← dans une conversation, le titre, × ; rend `Fermer` sur ×.
    fn tete(&mut self, ui: &egui::Ui, r: Rect) -> Option<Sortie> {
        let milieu = r.top() + 9.0 + 16.0;
        let mut x = r.left() + 14.0;
        let ruban = Rect::from_min_size(pos2(x, milieu - 10.0), vec2(5.0, 20.0));
        ui.painter().rect_filled(ruban, 3.0, couleurs::ACCENT);
        x = ruban.right() + 10.0;
        if self.ouverte.is_some() {
            let b = Rect::from_min_size(pos2(x, milieu - 16.0), vec2(32.0, 32.0));
            let nom = "Revenir à la liste des conversations";
            if bouton_carre(ui, b, id_de(Element::Retour), icones::RETOUR, "Toutes les conversations", nom).clicked() {
                self.retour();
            }
            x = b.right() + 10.0;
        }
        let titre = if self.ouverte.is_some() { "Conversation" } else { "Historique" };
        let gras = FontId::new(16.0, FontFamily::Name(natif::GRAS.into()));
        texte(ui.painter(), pos2(x, milieu), Align2::LEFT_CENTER, titre, gras, couleurs::ENCRE, 0.32);
        let b = Rect::from_min_size(pos2(r.right() - 9.0 - 32.0, milieu - 16.0), vec2(32.0, 32.0));
        let fermer =
            bouton_carre(ui, b, id_de(Element::Fermer), natif::icones::NON, "Fermer (Échap)", "Fermer l'historique");
        fermer.clicked().then_some(Sortie::Fermer)
    }

    /// La liste (H2 à H4) : la recherche, fixe en haut, puis les conversations par jour, qui défilent.
    fn liste(&mut self, ui: &mut egui::Ui, corps: Rect, temps: f64, aujourdhui: &str) {
        let champ = Rect::from_min_size(corps.min + vec2(MARGE, 10.0), vec2(corps.width() - 2.0 * MARGE, 38.0));
        self.champ_de_recherche(ui, champ, temps);
        let zone =
            Rect::from_min_max(pos2(corps.left() + MARGE, champ.bottom() + 2.0), corps.max - vec2(MARGE - BARRE, 12.0));
        let mut ouvrir = None;
        let mut defile = egui::ScrollArea::vertical().id_salt(id_de(Element::Liste)).auto_shrink([false, false]);
        if std::mem::take(&mut self.remonter_liste) {
            defile = defile.vertical_scroll_offset(0.0);
        }
        ui.scope_builder(UiBuilder::new().max_rect(zone), |ui| {
            defile.show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if let Some(avis) = self.avis {
                    message(ui, avis);
                }
                let Some(liste) = &self.liste else {
                    match self.attendue {
                        Some(t) if temps - t > ROUE => {
                            ui.add_space(24.0);
                            ui.vertical_centered(|ui| ui.add(egui::Spinner::new().size(18.0).color(couleurs::ENCRE_3)));
                        }
                        Some(t) => ui.ctx().request_repaint_after(Duration::from_secs_f64(t + ROUE - temps)),
                        None => {}
                    }
                    return;
                };
                if let Some(m) = message_de_la_liste(liste) {
                    message(ui, &m);
                }
                let mut jour = None;
                for c in liste.conversations.iter().flatten() {
                    if jour != Some(jour_de(c)) {
                        jour = Some(jour_de(c));
                        en_tete_du_jour(ui, &nom_du_jour(jour_de(c), aujourdhui));
                    }
                    if element(ui, c).clicked() {
                        ouvrir = Some(c.id.clone());
                    }
                }
                ui.add_space(4.0);
            });
        });
        if let Some(id) = ouvrir {
            self.demander_la_conversation(id);
        }
    }

    /// Le champ de recherche (`.hist-cherche`, H3) : la loupe, le texte, × pour l'effacer quand il y a quelque chose ;
    /// cerné de l'accent quand on y écrit. Chaque frappe relance la recherche 220 ms plus tard.
    fn champ_de_recherche(&mut self, ui: &mut egui::Ui, rect: Rect, temps: f64) {
        let id = id_de(Element::Cherche);
        let actif = ui.ctx().memory(|m| m.has_focus(id));
        let icones_de_windows = FontFamily::Name(natif::ICONES.into());
        let p = ui.painter();
        if actif {
            p.rect_filled(rect.expand(3.0), 12.0, couleurs::ACCENT.gamma_multiply(0.16));
        }
        let bord = if actif { couleurs::ACCENT } else { couleurs::FILET_FORT };
        p.rect(rect, 9.0, couleurs::FOND, Stroke::new(1.0, bord), StrokeKind::Inside);
        let loupe = FontId::new(13.0, icones_de_windows.clone());
        p.text(
            pos2(rect.left() + 12.0, rect.center().y),
            Align2::LEFT_CENTER,
            icones::CHERCHER,
            loupe,
            couleurs::ENCRE_3,
        );
        let effacer = !self.recherche.is_empty();
        let droite = rect.right() - if effacer { 34.0 } else { 10.0 };
        let zone = Rect::from_min_max(pos2(rect.left() + 34.0, rect.top()), pos2(droite, rect.bottom()));
        let champ = ui.put(
            zone,
            egui::TextEdit::singleline(&mut self.recherche)
                .id(id)
                .frame(egui::Frame::new())
                .margin(Margin::ZERO)
                .align(Align2::LEFT_CENTER)
                .font(FontId::proportional(14.0))
                .text_color(couleurs::ENCRE)
                .hint_text(RichText::new("Chercher dans les conversations…").color(couleurs::ENCRE_3))
                .desired_width(zone.width()),
        );
        if champ.changed() {
            self.frappe = Some(temps);
        }
        if self.focaliser && !ui.is_sizing_pass() {
            champ.request_focus();
            self.focaliser = false;
        }
        if effacer {
            let r = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), vec2(24.0, 24.0));
            let rep = ui.interact(r, id_de(Element::Effacer), Sense::click());
            let p = ui.painter();
            if rep.hovered() {
                p.circle_filled(r.center(), 11.0, couleurs::SURFACE_2);
            }
            let encre = if rep.hovered() { couleurs::ENCRE } else { couleurs::ENCRE_3 };
            p.text(r.center(), Align2::CENTER_CENTER, natif::icones::NON, FontId::new(10.0, icones_de_windows), encre);
            anneau(ui, &rep, r, 12.0);
            rep.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Effacer la recherche"));
            if rep.on_hover_text("Effacer la recherche").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.recherche.clear();
                self.frappe = Some(temps);
                champ.request_focus();
            }
        }
    }

    /// Une conversation (H5) : sa date et ses échanges, en bulles comme le fil (la personne, Eve et ses cartes, ce qui
    /// a mal fini) ; dessous, toujours en vue, « Reprendre cette conversation » (H7).
    fn conversation(
        &mut self,
        ui: &mut egui::Ui,
        corps: Rect,
        cartes: &mut Cartes,
        aujourdhui: &str,
    ) -> Option<Sortie> {
        let mut defile = egui::ScrollArea::vertical().id_salt(id_de(Element::Echanges)).auto_shrink([false, false]);
        if std::mem::take(&mut self.remonter_echanges) {
            defile = defile.vertical_scroll_offset(0.0);
        }
        let ouverte = self.ouverte.as_ref()?;
        let premier = ouverte.echanges.first()?;
        let pied = Rect::from_min_max(pos2(corps.left(), corps.bottom() - PIED), corps.max);
        let droite = corps.right() - (MARGE - BARRE);
        let zone = Rect::from_min_max(pos2(corps.left() + MARGE, corps.top()), pos2(droite, pied.top()));
        let mut gestes = Vec::new();
        ui.scope_builder(UiBuilder::new().max_rect(zone), |ui| {
            defile.show(ui, |ui| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(2.0);
                    let titre = titre_de_la_conversation(&ouverte.echanges, aujourdhui);
                    ui.add(egui::Label::new(natif::semi(titre, 13.0).color(couleurs::ENCRE_2)).selectable(false));
                });
                ui.add_space(10.0);
                for e in &ouverte.echanges {
                    let heure = heure_dans_la_conversation(e, premier);
                    let pieces: Vec<&str> = e.pieces.iter().map(String::as_str).collect();
                    let toi = Ligne {
                        de: "toi",
                        texte: &e.demande,
                        heure: &heure,
                        ancien: false,
                        pieces: &pieces,
                        en_cours: false,
                        tour: "",
                    };
                    natif::bulle(ui, &toi, cartes, &mut gestes);
                    let reponse = if e.reponse.trim().is_empty() { "(sans réponse)" } else { e.reponse.as_str() };
                    let eve = Ligne {
                        de: "eve",
                        texte: reponse,
                        heure: "",
                        ancien: false,
                        pieces: &[],
                        en_cours: false,
                        tour: "",
                    };
                    natif::bulle(ui, &eve, cartes, &mut gestes);
                    if !e.issue.is_empty() {
                        issue(ui, &e.issue);
                    }
                }
            });
        });
        ui.painter().hline(corps.x_range(), pied.top() + 0.5, Stroke::new(1.0, couleurs::FILET));
        let bouton = Rect::from_min_max(pied.min + vec2(MARGE, 11.0), pied.max - vec2(MARGE, 12.0));
        if bouton_reprendre(ui, bouton).clicked() {
            return Some(Sortie::Reprendre(ouverte.id.clone()));
        }
        gestes.into_iter().next().map(Sortie::Geste)
    }
}

// ---- Le fil qui lit ------------------------------------------------------------------------------------------------

/// Ce que le panneau demande au fil.
enum Demande {
    Liste { n: u64, qui: String, recherche: String },
    Conversation { n: u64, qui: String, id: String },
}

/// Ce que le fil rend ; `None` : la lecture a échoué (elle a paniqué).
enum Reponse {
    Liste { n: u64, recherche: String, conversations: Option<Vec<Resume>> },
    Conversation { n: u64, id: String, echanges: Option<Vec<Entree>> },
}

enum Lecteur {
    /// Pas encore lancé : il lui faut le contexte d'egui, pour réveiller la fenêtre quand il a lu.
    PasLance,
    Lance {
        envoi: Sender<Demande>,
        recu: Receiver<Reponse>,
    },
    /// Le fil ne part pas, ou s'est arrêté : plus rien ne se lit.
    Arrete,
}

impl Lecteur {
    /// Le fil vit autant que le panneau : il s'arrête quand celui-ci, fermé, ne lui demande plus rien.
    fn lancer(local: Option<PathBuf>, ctx: egui::Context) -> Lecteur {
        let (envoi, demandes) = mpsc::channel();
        let (reponses, recu) = mpsc::channel();
        let fil = thread::Builder::new().name("historique".into()).spawn(move || {
            // Le dossier local d'Eve se demande ici : même son `create_dir_all` reste hors de l'image.
            let local = local.unwrap_or_else(crate::reglages::dossier_local);
            lire(&local, &demandes, &reponses, &ctx);
        });
        if fil.is_ok() { Lecteur::Lance { envoi, recu } } else { Lecteur::Arrete }
    }
}

/// Les demandes, lues à leur tour ; de celles qui attendent, seule la dernière de chaque sorte se lit (une recherche
/// tapée vite n'en lit qu'une).
fn lire(local: &Path, demandes: &Receiver<Demande>, reponses: &Sender<Reponse>, ctx: &egui::Context) {
    while let Ok(premiere) = demandes.recv() {
        let file: Vec<Demande> = std::iter::once(premiere).chain(demandes.try_iter()).collect();
        for demande in dernieres(file) {
            let reponse = match demande {
                Demande::Liste { n, qui, recherche } => {
                    let lu = panic::catch_unwind(|| historique::conversations_dans(local, &qui, &recherche, LIMITE));
                    Reponse::Liste { n, recherche, conversations: lu.ok() }
                }
                Demande::Conversation { n, qui, id } => {
                    let lu = panic::catch_unwind(|| historique::conversation_dans(local, &qui, &id));
                    Reponse::Conversation { n, id, echanges: lu.ok() }
                }
            };
            if reponses.send(reponse).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    }
}

/// De la file des demandes, la dernière de chaque sorte, dans leur ordre.
fn dernieres(file: Vec<Demande>) -> Vec<Demande> {
    let derniere = |liste: bool| file.iter().rposition(|d| matches!(d, Demande::Liste { .. }) == liste);
    let (liste, conversation) = (derniere(true), derniere(false));
    file.into_iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) == liste || Some(*i) == conversation)
        .map(|(_, d)| d)
        .collect()
}

// ---- Les jours, les plages, les messages ---------------------------------------------------------------------------

/// « 2026-10-21 » : (2026, 10, 21) ; `None` pour ce qui n'est pas une date.
fn lire_date(iso: &str) -> Option<(i64, usize, i64)> {
    let mut p = iso.trim().split('-');
    let (a, m, j) = (p.next()?, p.next()?, p.next()?);
    if p.next().is_some() || a.len() != 4 || m.len() != 2 || j.len() != 2 {
        return None;
    }
    let (a, m, j): (i64, usize, i64) = (a.parse().ok()?, m.parse().ok()?, j.parse().ok()?);
    ((1..=12).contains(&m) && (1..=31).contains(&j)).then_some((a, m, j))
}

/// Le numéro d'un jour depuis le 1er janvier 1970 (le calendrier civil de Howard Hinnant, comme `journal.rs`).
fn numero_du_jour(a: i64, m: usize, j: i64) -> i64 {
    let m = m as i64;
    let a = if m <= 2 { a - 1 } else { a };
    let ere = a.div_euclid(400);
    let yoe = a - ere * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + j - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    ere * 146_097 + doe - 719_468
}

/// Le quantième, « 1er » pour le premier du mois, comme on l'écrit en français (`veilleur.rs` aussi).
fn quantieme(j: i64) -> String {
    if j == 1 { "1er".into() } else { j.to_string() }
}

/// Le jour d'une date : « Aujourd'hui », « Hier », « mercredi 21 octobre » (l'année, si ce n'est pas celle
/// d'aujourd'hui) ; ce qui n'est pas une date reste tel quel.
fn nom_du_jour(iso: &str, aujourdhui: &str) -> String {
    let (Some((a, m, j)), Some((a0, m0, j0))) = (lire_date(iso), lire_date(aujourdhui)) else { return iso.to_string() };
    let n = numero_du_jour(a, m, j);
    match numero_du_jour(a0, m0, j0) - n {
        0 => "Aujourd'hui".into(),
        1 => "Hier".into(),
        _ => {
            // Le 1er janvier 1970, le jour zéro, était un jeudi.
            let semaine = JOURS[(n + 4).rem_euclid(7) as usize];
            let annee = if a == a0 { String::new() } else { format!(" {a}") };
            format!("{semaine} {} {}{annee}", quantieme(j), MOIS[m - 1])
        }
    }
}

/// « 21 oct. », pour une conversation qui court sur deux jours.
fn jour_court(iso: &str) -> String {
    match lire_date(iso) {
        Some((_, m, j)) => format!("{} {}", quantieme(j), MOIS_COURTS[m - 1]),
        None => iso.to_string(),
    }
}

/// Le jour où une conversation se range : celui de son dernier échange.
fn jour_de(c: &Resume) -> &str {
    if c.fin_date.is_empty() { &c.date } else { &c.fin_date }
}

/// La plage d'une conversation : « 08:01 – 09:40 », « 08:01 » pour un seul moment, « 21 oct. 08:01 → 22 oct. 10:02 »
/// pour une conversation reprise un autre jour.
fn plage(c: &Resume) -> String {
    if !c.fin_date.is_empty() && c.fin_date != c.date {
        return format!("{} {} → {} {}", jour_court(&c.date), c.debut, jour_court(&c.fin_date), c.fin);
    }
    if !c.fin.is_empty() && c.fin != c.debut { format!("{} – {}", c.debut, c.fin) } else { c.debut.clone() }
}

/// « 1 échange », « 3 échanges ».
fn nombre_d_echanges(n: usize) -> String {
    format!("{n} échange{}", if n > 1 { "s" } else { "" })
}

/// Ce que dit la liste quand elle n'a rien à montrer (H4).
fn message_de_la_liste(liste: &Liste) -> Option<String> {
    match &liste.conversations {
        None => Some(ILLISIBLE.into()),
        Some(c) if !c.is_empty() => None,
        Some(_) if liste.recherche.is_empty() => Some(SANS_HISTORIQUE.into()),
        Some(_) => Some(format!("Rien ne correspond à « {} ».", liste.recherche)),
    }
}

/// La ligne en tête d'une conversation ouverte : « Aujourd'hui, 09:14 · 2 échanges ».
fn titre_de_la_conversation(echanges: &[Entree], aujourdhui: &str) -> String {
    let Some(premier) = echanges.first() else { return String::new() };
    format!("{}, {} · {}", nom_du_jour(&premier.date, aujourdhui), premier.heure, nombre_d_echanges(echanges.len()))
}

/// L'extrait de ce qu'on cherche, lisible : sans les marques du texte d'Eve (`**`, les accents graves, « --- »), et
/// coupé avant une carte ou un bloc de code, dont le JSON n'a rien à dire dans une liste. La page le montrait brut.
fn extrait_lisible(x: &str) -> String {
    let (avant, coupe) = match x.find("```") {
        Some(i) => (&x[..i], true),
        None => (x, false),
    };
    let mut s = avant.replace("**", "").replace('`', "");
    s = s.split_whitespace().filter(|m| *m != "---").collect::<Vec<_>>().join(" ");
    if coupe && !s.is_empty() {
        s.push_str(" …");
    }
    s
}

/// L'heure d'une demande dans sa conversation, et son jour s'il n'est pas celui du premier échange.
fn heure_dans_la_conversation(e: &Entree, premier: &Entree) -> String {
    if e.date == premier.date { e.heure.clone() } else { format!("{} {}", jour_court(&e.date), e.heure) }
}

// ---- Le dessin -----------------------------------------------------------------------------------------------------

/// Le fond d'un élément survolé (`--accent-wash`) : l'accent à 12 % sur la surface du panneau.
fn lavis() -> Color32 {
    melange(couleurs::SURFACE, couleurs::ACCENT, 0.12)
}

/// Un texte posé à `pos` selon `ancre`, ses lettres espacées de `espace`.
fn texte(p: &egui::Painter, pos: egui::Pos2, ancre: Align2, t: &str, police: FontId, couleur: Color32, espace: f32) {
    let format = TextFormat { font_id: police, color: couleur, extra_letter_spacing: espace, ..Default::default() };
    let galley = p.layout_job(LayoutJob::single_section(t.to_string(), format));
    p.galley(ancre.anchor_size(pos, galley.size()).min, galley, couleur);
}

/// Un texte sur une seule ligne, coupé d'un « … » s'il dépasse `largeur` (`text-overflow: ellipsis`) ; ses retours à la
/// ligne deviennent des espaces.
fn une_ligne(p: &egui::Painter, t: &str, police: FontId, couleur: Color32, largeur: f32) -> Arc<Galley> {
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut job = LayoutJob::single_section(t, TextFormat { font_id: police, color: couleur, ..Default::default() });
    job.wrap = TextWrapping { max_width: largeur, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    p.layout_job(job)
}

/// L'anneau du focus, quand on vient à un élément au clavier (la page : `:focus-visible`, 2 points d'accent). Il se
/// peint en dedans : la zone qui défile rognerait un anneau tracé autour d'une conversation de la liste.
fn anneau(ui: &egui::Ui, rep: &egui::Response, rect: Rect, rayon: f32) {
    if rep.has_focus() {
        ui.painter().rect_stroke(rect, rayon, Stroke::new(2.0, couleurs::ACCENT), StrokeKind::Inside);
    }
}

/// Un bouton carré de la tête (←, ×), comme `.fermer` dans la page : rien autour, un fond et un filet au survol.
fn bouton_carre(ui: &egui::Ui, rect: Rect, id: Id, code: char, infobulle: &str, nom: &'static str) -> egui::Response {
    let rep = ui.interact(rect, id, Sense::click());
    let (fond, bord, encre) = if rep.hovered() {
        (couleurs::SURFACE_2, couleurs::FILET, couleurs::ENCRE)
    } else {
        (Color32::TRANSPARENT, Color32::TRANSPARENT, couleurs::ENCRE_2)
    };
    let p = ui.painter();
    p.rect(rect, 8.0, fond, Stroke::new(1.0, bord), StrokeKind::Inside);
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        code,
        FontId::new(14.0, FontFamily::Name(natif::ICONES.into())),
        encre,
    );
    anneau(ui, &rep, rect, 8.0);
    rep.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, nom));
    rep.on_hover_text(infobulle).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// « Reprendre cette conversation » (`.hist-reprendre`) : toute la largeur, l'accent pour filet et pour encre.
fn bouton_reprendre(ui: &egui::Ui, rect: Rect) -> egui::Response {
    let rep = ui.interact(rect, id_de(Element::Reprendre), Sense::click());
    let enfonce = rep.is_pointer_button_down_on();
    let fond = match (enfonce, rep.hovered()) {
        (true, _) => melange(couleurs::SURFACE, couleurs::ACCENT, 0.2),
        (false, true) => lavis(),
        _ => couleurs::SURFACE,
    };
    let p = ui.painter();
    p.rect(rect, 9.0, fond, Stroke::new(1.0, couleurs::ACCENT), StrokeKind::Inside);
    // Enfoncé, il descend d'un point, comme `.bouton:active`.
    let centre = rect.center() + vec2(0.0, if enfonce { 1.0 } else { 0.0 });
    let semi = FontId::new(14.0, FontFamily::Name(natif::SEMI.into()));
    p.text(centre, Align2::CENTER_CENTER, "Reprendre cette conversation", semi, couleurs::ACCENT);
    anneau(ui, &rep, rect, 9.0);
    rep.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Reprendre cette conversation"));
    rep.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Le jour d'un groupe de conversations (`.hist-jour`) : en capitales, à chasse fixe.
fn en_tete_du_jour(ui: &mut egui::Ui, jour: &str) {
    ui.add_space(12.0);
    let format = TextFormat {
        font_id: FontId::monospace(11.0),
        color: couleurs::ENCRE_3,
        extra_letter_spacing: 0.66,
        ..Default::default()
    };
    let galley = ui.painter().layout_job(LayoutJob::single_section(jour.to_uppercase(), format));
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), galley.size().y), Sense::hover());
    ui.painter().galley(pos2(r.left() + 2.0, r.top()), galley, couleurs::ENCRE_3);
    ui.add_space(5.0);
}

/// Une conversation de la liste (`.hist-item`) : son titre, sa plage et ses échanges, l'extrait de ce qu'on cherche ;
/// au survol, le filet et le fond de l'accent.
fn element(ui: &mut egui::Ui, c: &Resume) -> egui::Response {
    let largeur = ui.available_width();
    let dedans = (largeur - 2.0 * 12.0).max(40.0);
    let titre = if c.titre.trim().is_empty() { "(sans demande)" } else { c.titre.as_str() };
    let meta = format!("{} · {}", plage(c), nombre_d_echanges(c.echanges));
    let (g_titre, g_meta, g_extrait) = {
        let p = ui.painter();
        let semi = FontId::new(14.0, FontFamily::Name(natif::SEMI.into()));
        let extrait = c.extrait.as_deref().map(extrait_lisible).filter(|x| !x.is_empty()).map(|x| {
            let format = TextFormat {
                font_id: FontId::proportional(12.5),
                color: couleurs::ENCRE_2,
                italics: true,
                line_height: Some(17.5),
                ..Default::default()
            };
            let mut job = LayoutJob::simple_format(x, format);
            job.wrap.max_width = dedans;
            p.layout_job(job)
        });
        (
            une_ligne(p, titre, semi, couleurs::ENCRE, dedans),
            une_ligne(p, &meta, FontId::proportional(12.0), couleurs::ENCRE_3, dedans),
            extrait,
        )
    };
    let hauteur = 1.0
        + 8.0
        + g_titre.size().y
        + 2.0
        + g_meta.size().y
        + g_extrait.as_ref().map_or(0.0, |g| 3.0 + g.size().y)
        + 8.0
        + 1.0;
    let (rect, _) = ui.allocate_exact_size(vec2(largeur, hauteur), Sense::hover());
    let rep = ui.interact(rect, id_de(Element::Conversation(&c.id)), Sense::click());
    if ui.is_rect_visible(rect) {
        let (fond, bord) =
            if rep.hovered() { (lavis(), couleurs::ACCENT) } else { (couleurs::SURFACE_2, couleurs::FILET) };
        let p = ui.painter();
        p.rect(rect, 10.0, fond, Stroke::new(1.0, bord), StrokeKind::Inside);
        let x = rect.left() + 12.0;
        let mut y = rect.top() + 9.0;
        let h = g_titre.size().y;
        p.galley(pos2(x, y), g_titre, couleurs::ENCRE);
        y += h + 2.0;
        let h = g_meta.size().y;
        p.galley(pos2(x, y), g_meta, couleurs::ENCRE_3);
        if let Some(g) = g_extrait {
            p.galley(pos2(x, y + h + 3.0), g, couleurs::ENCRE_2);
        }
        anneau(ui, &rep, rect, 10.0);
    }
    ui.add_space(5.0);
    let nom = format!("{titre} — {meta}");
    rep.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &nom));
    rep.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Un message de la liste (`.hist-vide`) : centré, pâle, à 20 points de ce qui l'entoure.
fn message(ui: &mut egui::Ui, texte: &str) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.set_max_width(ui.available_width() - 12.0);
        ui.add(egui::Label::new(RichText::new(texte).size(13.5).color(couleurs::ENCRE_3)).selectable(false));
    });
    ui.add_space(20.0);
}

/// Ce qui a mal fini (`.hist-issue`), juste sous la bulle d'Eve, à l'aplomb de son bord (après sa petite orbe). La
/// bulle laisse 20 points sous elle : l'avis remonte à 4 points, comme dans la page.
fn issue(ui: &mut egui::Ui, sorte: &str) {
    let texte = if sorte == "interrompue" { "Réponse interrompue." } else { "Le tour s'est mal terminé." };
    ui.add_space(-16.0);
    ui.horizontal(|ui| {
        ui.add_space(34.0);
        ui.add(egui::Label::new(RichText::new(texte).size(11.5).color(couleurs::ENCRE_3)).selectable(false));
    });
    ui.add_space(2.0);
}

#[cfg(test)]
mod essais {
    use super::*;

    fn resume(id: &str, date: &str, debut: &str, fin_date: &str, fin: &str, echanges: usize) -> Resume {
        Resume {
            id: id.into(),
            date: date.into(),
            debut: debut.into(),
            fin_date: fin_date.into(),
            fin: fin.into(),
            echanges,
            titre: "Une demande".into(),
            extrait: None,
        }
    }

    fn entree(date: &str, heure: &str) -> Entree {
        Entree {
            conversation: "c".into(),
            date: date.into(),
            heure: heure.into(),
            demande: "Une demande".into(),
            reponse: "Une réponse".into(),
            pieces: Vec::new(),
            issue: String::new(),
        }
    }

    #[test]
    fn les_jours_se_nomment_comme_dans_la_page() {
        let aujourdhui = "2026-10-24";
        assert_eq!(nom_du_jour("2026-10-24", aujourdhui), "Aujourd'hui");
        assert_eq!(nom_du_jour("2026-10-23", aujourdhui), "Hier");
        assert_eq!(nom_du_jour("2026-10-21", aujourdhui), "mercredi 21 octobre");
        assert_eq!(nom_du_jour("2026-11-01", aujourdhui), "dimanche 1er novembre");
        assert_eq!(nom_du_jour("2025-12-18", aujourdhui), "jeudi 18 décembre 2025", "l'année d'une autre année");
        // Le 1er janvier : hier, d'une autre année.
        assert_eq!(nom_du_jour("2025-12-31", "2026-01-01"), "Hier");
        assert_eq!(nom_du_jour("2028-02-29", "2028-03-02"), "mardi 29 février", "2028 est bissextile");
        assert_eq!(nom_du_jour("sans-date", aujourdhui), "sans-date");
        assert_eq!(jour_court("2026-10-21"), "21 oct.");
        assert_eq!(jour_court("2026-07-01"), "1er juill.");
        assert_eq!(jour_court("2026-13-01"), "2026-13-01");
    }

    #[test]
    fn la_plage_et_le_nombre_d_echanges() {
        assert_eq!(plage(&resume("a", "2026-10-21", "08:01", "2026-10-21", "09:40", 3)), "08:01 – 09:40");
        assert_eq!(plage(&resume("a", "2026-10-21", "08:01", "2026-10-21", "08:01", 1)), "08:01");
        assert_eq!(
            plage(&resume("a", "2026-10-21", "08:01", "2026-10-22", "10:02", 2)),
            "21 oct. 08:01 → 22 oct. 10:02",
            "une conversation reprise un autre jour"
        );
        assert_eq!(nombre_d_echanges(1), "1 échange");
        assert_eq!(nombre_d_echanges(2), "2 échanges");
        assert_eq!(
            extrait_lisible("Trois factures pour **2 418,96 $** en tout. --- ```carte {\"type\":\"tableau\"} …"),
            "Trois factures pour 2 418,96 $ en tout. …"
        );
        assert_eq!(extrait_lisible("… rangée dans `Chantier/Photos`. …"), "… rangée dans Chantier/Photos. …");
        // Elle se range au jour de son dernier échange.
        assert_eq!(jour_de(&resume("a", "2026-10-21", "08:01", "2026-10-22", "10:02", 2)), "2026-10-22");
        assert_eq!(jour_de(&resume("a", "2026-10-21", "08:01", "", "", 1)), "2026-10-21");
    }

    #[test]
    fn les_messages_de_la_liste() {
        let liste =
            |recherche: &str, conversations: Option<Vec<Resume>>| Liste { recherche: recherche.into(), conversations };
        assert_eq!(message_de_la_liste(&liste("", Some(Vec::new()))).as_deref(), Some(SANS_HISTORIQUE));
        assert_eq!(
            message_de_la_liste(&liste("facture", Some(Vec::new()))).as_deref(),
            Some("Rien ne correspond à « facture ».")
        );
        assert_eq!(message_de_la_liste(&liste("", None)).as_deref(), Some(ILLISIBLE));
        let une = vec![resume("a", "2026-10-21", "08:01", "2026-10-21", "08:01", 1)];
        assert_eq!(message_de_la_liste(&liste("", Some(une))), None);
    }

    #[test]
    fn une_conversation_dit_son_jour_et_l_heure_de_chaque_demande() {
        let (premier, suivant) = (entree("2026-10-21", "08:01"), entree("2026-10-22", "10:02"));
        let echanges = [premier.clone(), entree("2026-10-21", "08:03"), suivant.clone()];
        assert_eq!(titre_de_la_conversation(&echanges, "2026-10-24"), "mercredi 21 octobre, 08:01 · 3 échanges");
        assert_eq!(titre_de_la_conversation(&echanges[..1], "2026-10-21"), "Aujourd'hui, 08:01 · 1 échange");
        assert_eq!(heure_dans_la_conversation(&echanges[1], &premier), "08:03");
        assert_eq!(heure_dans_la_conversation(&suivant, &premier), "22 oct. 10:02", "un autre jour le dit");
    }

    #[test]
    fn une_reponse_depassee_est_jetee() {
        let mut h = Historique::nouveau();
        h.qui = Some("Essai".into());
        h.lecteur = Lecteur::Arrete;
        (h.demande_liste, h.attendue) = (3, Some(0.0));
        let liste = |n, recherche: &str| Reponse::Liste {
            n,
            recherche: recherche.into(),
            conversations: Some(vec![resume(recherche, "2026-10-21", "08:01", "2026-10-21", "08:01", 1)]),
        };
        h.recevoir(liste(2, "fac"));
        assert!(h.liste.is_none() && h.attendue.is_some(), "la réponse d'une demande d'avant ne s'affiche pas");
        h.recevoir(liste(3, "facture"));
        assert_eq!(h.liste.as_ref().map(|l| l.recherche.as_str()), Some("facture"));
        assert!(h.attendue.is_none());
        // Une conversation demandée, puis le retour à la liste : elle n'ouvre plus rien en arrivant.
        h.demande_conversation = 1;
        h.conversation_attendue = true;
        h.retour();
        let arrivee =
            Reponse::Conversation { n: 1, id: "c".into(), echanges: Some(vec![entree("2026-10-21", "08:01")]) };
        h.recevoir(arrivee);
        assert!(h.ouverte.is_none());
        // La dernière demandée s'ouvre ; vide ou illisible, la liste le dit.
        h.demande_conversation = 5;
        h.recevoir(Reponse::Conversation { n: 5, id: "c".into(), echanges: Some(Vec::new()) });
        assert_eq!(h.avis, Some(CONVERSATION_VIDE));
        h.recevoir(Reponse::Conversation { n: 5, id: "c".into(), echanges: None });
        assert_eq!(h.avis, Some(CONVERSATION_ILLISIBLE));
        h.recevoir(Reponse::Conversation { n: 5, id: "c".into(), echanges: Some(vec![entree("2026-10-21", "08:01")]) });
        assert!(h.ouverte.is_some() && h.avis.is_none());
        // Échap : de la conversation à la liste, puis la fenêtre ferme le panneau.
        assert!(!h.echap());
        assert!(h.ouverte.is_none() && h.focaliser);
        assert!(h.echap());
    }

    #[test]
    fn seule_la_derniere_demande_de_chaque_sorte_se_lit() {
        let liste = |n| Demande::Liste { n, qui: "Essai".into(), recherche: String::new() };
        let conversation = |n| Demande::Conversation { n, qui: "Essai".into(), id: "c".into() };
        let numeros = |file: Vec<Demande>| -> Vec<(bool, u64)> {
            dernieres(file)
                .into_iter()
                .map(|d| match d {
                    Demande::Liste { n, .. } => (true, n),
                    Demande::Conversation { n, .. } => (false, n),
                })
                .collect()
        };
        assert_eq!(numeros(vec![liste(1), liste(2), conversation(1), liste(3)]), [(false, 1), (true, 3)]);
        assert_eq!(numeros(vec![conversation(4)]), [(false, 4)]);
        assert!(numeros(Vec::new()).is_empty());
    }

    #[test]
    fn le_fil_lit_l_historique_d_un_dossier_local() {
        let local = std::env::temp_dir().join(format!("eve-historique-ui-{}", std::process::id()));
        let dossier = historique::dossier_dans(&local, "Essai");
        std::fs::create_dir_all(&dossier).unwrap();
        let mut e = entree("2026-10-21", "08:01");
        e.conversation = "2026-10-21 08-00-00".into();
        e.demande = "La facture de Martin est-elle entrée ?".into();
        std::fs::write(dossier.join("2026-10.jsonl"), serde_json::to_string(&e).unwrap() + "\n").unwrap();
        let (envoi, demandes) = mpsc::channel();
        let (reponses, recu) = mpsc::channel();
        envoi.send(Demande::Liste { n: 1, qui: "Essai".into(), recherche: "fact".into() }).unwrap();
        envoi.send(Demande::Conversation { n: 1, qui: "Essai".into(), id: "2026-10-21 08-00-00".into() }).unwrap();
        drop(envoi);
        lire(&local, &demandes, &reponses, &egui::Context::default());
        let lues: Vec<Reponse> = recu.try_iter().collect();
        let _ = std::fs::remove_dir_all(&local);
        assert_eq!(lues.len(), 2);
        let Reponse::Liste { conversations: Some(c), .. } = &lues[0] else { panic!("la liste") };
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].extrait.as_deref(), Some("La facture de Martin est-elle entrée ?"));
        let Reponse::Conversation { echanges: Some(e), .. } = &lues[1] else { panic!("la conversation") };
        assert_eq!(e.len(), 1);
    }
}
