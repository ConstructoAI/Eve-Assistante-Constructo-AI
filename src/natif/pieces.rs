//! Les pièces jointes de la fenêtre native (le lot 6.3), comme celles de la page (eve.html : 337-348, 534-539,
//! 713-734, 757, 2517-2540, 2955-2966) : le trombone près de la saisie ouvre la boîte de dialogue de Windows, plusieurs
//! fichiers à la fois ; un ou plusieurs fichiers se glissent n'importe où sur la fenêtre, sous le voile pointillé du
//! dépôt ; les pièces en attente se montrent en pastilles au-dessus de la saisie, et leur × les retire. Un fichier se
//! joint par la fonction de la page ([`ecran::joindre`] : la copie dans `pieces\`, les 40 Mo, les refus et leurs mots),
//! dans un fil ; un refus va au chef, qui l'écrit au fil. Les pièces en attente sont celles que publie le chef : la
//! fenêtre n'en garde aucune. Les mêmes fonctions les dessinent dans les deux thèmes ; seul l'habit change ([`Habit`]).

use std::fs::File;
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;

use eframe::egui::text::{LayoutJob, TextFormat, TextWrapping};
use eframe::egui::{
    self, Align, Align2, Color32, FontFamily, FontId, Layout, Rect, Sense, Shape, Stroke, StrokeKind, pos2, vec2,
};
use serde_json::Value;

use super::theme::erp;
use super::{Fenetre, GRAS, ICONES, SEMI, couleurs, icones};
use crate::cartes::taille_lisible;
use crate::chef::Evt;
use crate::ecran::{self, PieceRefusee};

/// L'habit des pièces jointes dans un thème : ce qui change de l'un à l'autre — leurs couleurs, leurs rayons, la
/// taille du trombone ; leur forme, leurs mots et leurs gestes restent les mêmes.
pub(super) struct Habit {
    /// Une pastille : son fond, son filet, son rayon ; l'encre de son nom ; celle de sa taille, de son trombone et de
    /// son × ; son × survolé, son fond et son encre.
    fond: Color32,
    filet: Color32,
    rayon: u8,
    encre: Color32,
    discret: Color32,
    retirer_fond: Color32,
    retirer_encre: Color32,
    /// Le trombone : son côté, son rayon, son glyphe ; au repos, puis survolé, son fond, son filet, son encre.
    cote: f32,
    rayon_du_bouton: f32,
    glyphe: f32,
    bouton_fond: [Color32; 2],
    bouton_filet: [Color32; 2],
    bouton_encre: [Color32; 2],
    /// Le voile du dépôt : son fond, translucide ; l'encre de son pointillé et de ses mots.
    voile: Color32,
    accent: Color32,
}

/// Le thème sombre (le centre de commande, la conversation seule), dans les couleurs de l'Eve de Constructo AI : la
/// pastille en pilule comme celle de la page, son × rouge au survol (`--crit-wash`) ; le trombone rond et discret, à la
/// taille de l'envoi ; le voile, le fond de la fenêtre à 88 %.
pub(super) const SOMBRE: Habit = Habit {
    fond: couleurs::SURFACE_2,
    filet: couleurs::FILET,
    rayon: 255,
    encre: couleurs::ENCRE,
    discret: couleurs::ENCRE_3,
    retirer_fond: Color32::from_rgb(0x2A, 0x1D, 0x22),
    retirer_encre: couleurs::CRIT,
    cote: 36.0,
    rayon_du_bouton: 18.0,
    glyphe: 15.0,
    bouton_fond: [Color32::TRANSPARENT, Color32::from_rgba_unmultiplied_const(255, 255, 255, 14)],
    bouton_filet: [Color32::TRANSPARENT; 2],
    bouton_encre: [couleurs::ENCRE_2, couleurs::ENCRE],
    voile: Color32::from_rgba_unmultiplied_const(0x0F, 0x19, 0x23, 224),
    accent: couleurs::ACCENT,
};

/// Le thème « Constructo AI » : les fichiers joints de l'ERP (`FileUpload.tsx` : blancs, le filet #EDEBE9, un rayon
/// de 4, le rouge pâle du retrait) ; le trombone en bouton d'outil de la saisie de son assistant, comme le micro ; le
/// voile de sa zone de dépôt (`seaop-primary-50`), au bleu primaire.
pub(super) const CLAIR: Habit = Habit {
    fond: Color32::WHITE,
    filet: erp::FILET,
    rayon: 4,
    encre: erp::TEXTE,
    discret: erp::TEXTE_3,
    retirer_fond: Color32::from_rgb(0xFE, 0xF2, 0xF2),
    retirer_encre: Color32::from_rgb(0xEF, 0x44, 0x44),
    cote: 42.0,
    rayon_du_bouton: 4.0,
    glyphe: 17.0,
    bouton_fond: [Color32::from_rgba_unmultiplied_const(255, 255, 255, 190), Color32::WHITE],
    bouton_filet: [erp::FILET_BOUTON, erp::TEXTE_3],
    bouton_encre: [erp::TEXTE_2, erp::TEXTE],
    voile: Color32::from_rgba_unmultiplied_const(0xF0, 0xF6, 0xFF, 224),
    accent: erp::PRIMAIRE,
};

/// Le trombone, pour l'essai qui le clique.
pub(crate) fn id_du_trombone() -> egui::Id {
    egui::Id::unique("trombone-des-pieces")
}

/// Le × de la pièce `id`, pour l'essai qui le clique.
pub(crate) fn id_du_retrait(id: &str) -> egui::Id {
    egui::Id::unique(("retirer-la-piece", id))
}

impl Fenetre {
    /// Les pièces en attente (P4), au-dessus de la saisie : une pastille par pièce que publie le chef, à 6 points
    /// l'une de l'autre, à la ligne au besoin, dans la largeur moins `marge` de chaque côté ; rien s'il n'y en a pas.
    /// Leur × (P5) les retire (`piece_retirer`).
    pub(super) fn pieces_en_attente(&self, ui: &mut egui::Ui, habit: &Habit, marge: f32) {
        let Some(pieces) = self.etat["pieces"].as_array().filter(|p| !p.is_empty()) else { return };
        let place = ui.available_rect_before_wrap().shrink2(vec2(marge, 0.0));
        let rangees = Layout::left_to_right(Align::Min).with_main_wrap(true);
        ui.scope_builder(egui::UiBuilder::new().max_rect(place).layout(rangees), |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for p in pieces {
                let id = p["id"].as_str().unwrap_or("");
                if pastille(ui, id, p["nom"].as_str().unwrap_or(""), p["taille"].as_u64().unwrap_or(0), habit) {
                    self.geste("piece_retirer", Value::String(id.to_string()));
                }
            }
        });
        ui.add_space(8.0);
    }

    /// Le trombone (P1), près de la saisie : il ouvre la boîte de dialogue « Ouvrir » de Windows ; les fichiers qu'on y
    /// choisit se joignent comme ceux qu'on dépose.
    pub(super) fn trombone(&self, ui: &mut egui::Ui, habit: &Habit) {
        let (rect, _) = ui.allocate_exact_size(vec2(habit.cote, habit.cote), Sense::hover());
        let rep = ui.interact(rect, id_du_trombone(), Sense::click());
        let k = usize::from(rep.hovered());
        let p = ui.painter();
        let filet = Stroke::new(1.0, habit.bouton_filet[k]);
        p.rect(rect, habit.rayon_du_bouton, habit.bouton_fond[k], filet, StrokeKind::Inside);
        let police = FontId::new(habit.glyphe, FontFamily::Name(ICONES.into()));
        p.text(rect.center(), Align2::CENTER_CENTER, icones::PIECE, police, habit.bouton_encre[k]);
        rep.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Joindre un fichier"));
        let rep = rep.on_hover_text("Joindre un fichier (ou glisse-le dans la fenêtre)");
        if rep.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            choisir_puis_joindre(self.rapport.clone());
        }
    }

    /// Le dépôt (P2) : tant qu'un fichier se glisse sur la fenêtre, le voile par-dessus tout ; les fichiers déposés se
    /// joignent.
    pub(super) fn depot(&self, ctx: &egui::Context, habit: &Habit) {
        let (survol, deposes) = ctx.input(|i| {
            let deposes: Vec<PathBuf> = i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect();
            (!i.raw.hovered_files.is_empty(), deposes)
        });
        if survol {
            voile(ctx, habit);
        }
        if !deposes.is_empty() {
            joindre(deposes, self.rapport.clone());
        }
    }
}

/// Une pastille (P4, P5), comme celles de la page (`.piece`) : le trombone et le nom de la pièce (260 points au plus,
/// coupé d'un « … »), sa taille, plus petite ; à droite, son × dans un rond de 20 points (« Retirer » ; pour un lecteur
/// d'écran, « Retirer nom »). Vrai quand on clique son ×.
fn pastille(ui: &mut egui::Ui, id: &str, nom: &str, taille: u64, habit: &Habit) -> bool {
    let p = ui.painter();
    let icone = FontId::new(12.0, FontFamily::Name(ICONES.into()));
    let icone = p.layout_no_wrap(icones::PIECE.to_string(), icone, habit.discret);
    let poids = p.layout_no_wrap(taille_lisible(taille), FontId::proportional(10.5), habit.discret);
    // Autour du nom : 10 points à gauche, 5 après le trombone, 6 avant et après la taille, le rond du ×, 4 à droite.
    let autour = 10.0 + icone.size().x + 5.0 + 6.0 + poids.size().x + 6.0 + 20.0 + 4.0;
    let format = TextFormat::simple(FontId::new(12.5, FontFamily::Name(SEMI.into())), habit.encre);
    let mut job = LayoutJob::single_section(nom.to_string(), format);
    job.wrap = TextWrapping {
        max_width: (ui.available_width() - autour).clamp(24.0, 260.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let texte = p.layout_job(job);
    let (rect, _) = ui.allocate_exact_size(vec2(autour + texte.size().x, 26.0), Sense::hover());
    let croix = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(20.0, 20.0));
    let retirer = ui.interact(croix, id_du_retrait(id), Sense::click());
    retirer.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Retirer {nom}")));
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect(rect, habit.rayon, habit.fond, Stroke::new(1.0, habit.filet), StrokeKind::Inside);
        let mut x = rect.left() + 10.0;
        for (morceau, ecart) in [(icone, 5.0), (texte, 6.0), (poids, 0.0)] {
            let l = morceau.size().x;
            p.galley(pos2(x, rect.center().y - morceau.size().y / 2.0), morceau, habit.encre);
            x += l + ecart;
        }
        let survol = retirer.hovered();
        if survol {
            p.circle_filled(croix.center(), 10.0, habit.retirer_fond);
        }
        let encre = if survol { habit.retirer_encre } else { habit.discret };
        let police = FontId::new(9.0, FontFamily::Name(ICONES.into()));
        p.text(croix.center(), Align2::CENTER_CENTER, icones::NON, police, encre);
    }
    retirer.on_hover_text("Retirer").on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Le voile du dépôt (P2), comme celui de la page (`.depot`) : à 10 points des bords de la fenêtre, son fond, un
/// pointillé de 3 points aux coins de 16, et au milieu, en gras, « Dépose ici : le fichier part avec ta demande » ;
/// par-dessus tout, les réglages et l'historique compris.
fn voile(ctx: &egui::Context, habit: &Habit) {
    let rect = ctx.content_rect().shrink(10.0);
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("voile-du-depot")));
    p.rect_filled(rect, 16.0, habit.voile);
    let mut contour = Vec::new();
    let arrondi = egui::epaint::RoundedRect::new(rect.shrink(1.5), 14.5);
    egui::epaint::tessellator::path::rounded_rectangle(&mut contour, arrondi);
    if let Some(&debut) = contour.first() {
        contour.push(debut);
    }
    p.extend(Shape::dashed_line(&contour, Stroke::new(3.0, habit.accent), 9.0, 6.0));
    let police = FontId::new(18.0, FontFamily::Name(GRAS.into()));
    let texte = "Dépose ici : le fichier part avec ta demande".to_string();
    let mut mots = LayoutJob::simple(texte, police, habit.accent, rect.width() - 40.0);
    mots.halign = Align::Center;
    let mots = p.layout_job(mots);
    p.galley(pos2(rect.center().x, rect.center().y - mots.size().y / 2.0), mots, habit.accent);
}

/// Des fichiers choisis ou déposés, joints l'un après l'autre dans un fil : 40 Mo ne se copient pas le temps d'une
/// image.
fn joindre(chemins: Vec<PathBuf>, rapport: Sender<Evt>) {
    thread::spawn(move || joindre_tous(&chemins, &rapport));
}

/// Chaque fichier se joint par la fonction de la page ; un refus va au chef (`piece_erreur`), qui l'écrit au fil.
fn joindre_tous(chemins: &[PathBuf], rapport: &Sender<Evt>) {
    for chemin in chemins {
        let nom = chemin.file_name().map_or_else(|| chemin.display().to_string(), |n| n.to_string_lossy().into_owned());
        let joint = match File::open(chemin).and_then(|f| Ok((f.metadata()?.len(), f))) {
            Ok((taille, fichier)) => ecran::joindre(&nom, taille, fichier, rapport),
            Err(e) => Err(PieceRefusee::echec(&nom, e)),
        };
        if let Err(refus) = joint {
            let _ = rapport.send(Evt::Ui { action: "piece_erreur".into(), valeur: Value::String(refus.to_string()) });
        }
    }
}

/// Le trombone cliqué : la boîte de dialogue dans son propre fil, attachée à la fenêtre d'Eve, puis les fichiers
/// choisis, joints dans ce même fil ; si elle ne s'ouvre pas, le chef l'écrit au fil.
fn choisir_puis_joindre(rapport: Sender<Evt>) {
    let proprietaire = fenetre_d_eve();
    thread::spawn(move || match choisir(proprietaire) {
        Ok(chemins) => joindre_tous(&chemins, &rapport),
        Err(e) => {
            let valeur = Value::String(format!("La boîte de dialogue « Ouvrir » ne s'ouvre pas : {e}"));
            let _ = rapport.send(Evt::Ui { action: "piece_erreur".into(), valeur });
        }
    });
}

/// La fenêtre d'Eve, celle de sa barre de titre ([`crate::cadre`]) : la boîte de dialogue s'y attache, au-dessus d'elle
/// même quand Eve reste « toujours devant » ; 0 : à aucune.
#[cfg(windows)]
pub(crate) fn fenetre_d_eve() -> isize {
    crate::cadre::poignee().map_or(0, |h| h.0 as isize)
}

/// La boîte de dialogue « Ouvrir » de Windows (P1), celle qu'Edge ouvre pour la page : plusieurs fichiers à la fois,
/// modale au-dessus de la fenêtre `proprietaire` (0 : d'aucune). Elle tient dans le fil qui l'appelle, COM en
/// appartement (STA, ce qu'elle veut) : la fenêtre d'Eve, dans le sien, continue de se dessiner. Rend les fichiers
/// choisis ; rien si on l'annule ; ce qui l'empêche de s'ouvrir.
#[cfg(windows)]
fn choisir(proprietaire: isize) -> Result<Vec<PathBuf>, String> {
    use std::os::windows::ffi::OsStringExt;

    use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOS_ALLOWMULTISELECT, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    };

    // SAFETY : COM pour ce fil, défait à la fin si l'initialisation a réussi.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let choisis = || -> windows::core::Result<Vec<PathBuf>> {
        // SAFETY : la boîte de dialogue et ses résultats ne vivent que dans cette fermeture, avant que COM se défasse ;
        // chaque nom que rend Windows se libère par CoTaskMemFree, une fois lu.
        unsafe {
            let dialogue: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialogue
                .SetOptions(dialogue.GetOptions()? | FOS_ALLOWMULTISELECT | FOS_FORCEFILESYSTEM | FOS_NOCHANGEDIR)?;
            dialogue.Show((proprietaire != 0).then_some(HWND(proprietaire as *mut core::ffi::c_void)))?;
            let elements = dialogue.GetResults()?;
            let mut chemins = Vec::new();
            for k in 0..elements.GetCount()? {
                let nom = elements.GetItemAt(k)?.GetDisplayName(SIGDN_FILESYSPATH)?;
                chemins.push(PathBuf::from(std::ffi::OsString::from_wide(nom.as_wide())));
                CoTaskMemFree(Some(nom.0 as *const core::ffi::c_void));
            }
            Ok(chemins)
        }
    };
    let choisis = choisis();
    if com.is_ok() {
        // SAFETY : l'initialisation de ce fil a réussi ; ce qui était de COM s'en est allé avec la fermeture.
        unsafe { CoUninitialize() };
    }
    match choisis {
        // Annulée, elle rend une erreur : rien n'est choisi.
        Err(e) if e.code() == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) => Ok(Vec::new()),
        autre => autre.map_err(|e| e.message()),
    }
}

#[cfg(not(windows))]
pub(crate) fn fenetre_d_eve() -> isize {
    0
}

#[cfg(not(windows))]
fn choisir(_proprietaire: isize) -> Result<Vec<PathBuf>, String> {
    Ok(Vec::new())
}
