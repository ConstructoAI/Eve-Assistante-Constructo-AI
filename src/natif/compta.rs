//! L'onglet « Comptabilité » du tableau de bord (le lot 13.2) : les vues d'Entrepreneur Général sur la comptabilité de
//! l'ERP, lue en lecture seule ([`crate::compta`]) — le tableau de bord, les factures des fournisseurs, celles des
//! clients, les sommaires par mois, par fournisseur et par chantier, l'historique. En tête, le choix de la vue (en
//! segments, dans l'ordre des onglets d'EG), la période et le chantier, qui valent pour toutes les vues ; dessous, la
//! vue. Chaque vue a son fichier (`natif\compta\`) et son calcul (`compta\`).
//!
//! La lecture se fait à la demande : l'onglet qui se montre la demande au chef (le geste `compta_lire`), qui la fait au
//! plus une fois par intervalle et l'écrit dans `registre.json` ; le centre la relit ([`Centre::registre`]). Rien ne
//! s'écrit dans l'ERP d'ici : un clic sur une facture est une demande à Eve (« En parler à Eve »), jamais une écriture.
//!
//! Ce que la fenêtre retient de l'onglet — la vue, la période, le chantier, et un « aller à » d'une vue à l'autre (un
//! clic sur une barre du tableau de bord ouvre la liste filtrée) — vit dans la mémoire d'egui, le temps que la fenêtre
//! vit : rien ne s'écrit.
//!
//! [`Centre::registre`]: crate::centre::Centre::registre

mod bord;
mod historique;
mod listes;
mod sommaires;

use eframe::egui::{self, Align, Layout, Sense, UiBuilder, vec2};
use serde_json::Value;

use super::Fenetre;
use super::planning::etat_vide;
use super::tableau::{Liaison, Maintenant, PERIMES, Vue, de_quand, quand_dit};
use super::theme::{self, BoutonErp, erp};
use crate::compta::{self, Chantier, Cote, Filtres, Lecture, Periode, Registre};

// ---- Les vues -------------------------------------------------------------------------------------------------------

/// Les vues de l'onglet, dans l'ordre des onglets d'EG.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum VueCompta {
    #[default]
    Bord,
    Fournisseurs,
    Clients,
    ParMois,
    ParFournisseur,
    ParChantier,
    Historique,
}

/// Chaque vue : son nom, sa bulle.
const VUES: [(VueCompta, &str, &str); 7] = [
    (VueCompta::Bord, "Tableau de bord", "Les factures en graphiques ; touche une barre pour voir ses factures"),
    (VueCompta::Fournisseurs, "Fournisseurs", "Les factures reçues des fournisseurs"),
    (VueCompta::Clients, "Clients", "Les factures émises aux clients, et ce qui reste à recevoir"),
    (VueCompta::ParMois, "Par mois", "Les dépenses et les revenus, mois par mois"),
    (VueCompta::ParFournisseur, "Par fournisseur", "Les dépenses, fournisseur par fournisseur"),
    (VueCompta::ParChantier, "Par chantier", "Les dépenses, les revenus et la marge de chaque chantier"),
    (VueCompta::Historique, "Historique", "Le journal de la comptabilité : qui a fait quoi, et quand"),
];

/// Ce que l'onglet a choisi : la vue, la période et le chantier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Choix {
    pub(crate) vue: VueCompta,
    pub(crate) filtres: Filtres,
}

const CHOIX: &str = "compta-choix-de-l-onglet";
const ALLER: &str = "compta-aller-a";
const DEMANDEE: &str = "compta-lecture-demandee";

/// Ce que l'onglet a choisi (d'office : le tableau de bord, toute la période, tous les chantiers).
pub(crate) fn choix(ctx: &egui::Context) -> Choix {
    ctx.data(|d| d.get_temp::<Choix>(egui::Id::unique(CHOIX))).unwrap_or_default()
}

pub(crate) fn choisir(ctx: &egui::Context, c: Choix) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(CHOIX), c));
    ctx.request_repaint();
}

/// D'une vue à l'autre avec un filtre de liste : un clic sur une barre du tableau de bord (les factures d'un mois, d'un
/// corps de métier, d'un fournisseur…) ouvre la liste filtrée. La liste le prend une fois ([`prendre_l_aller`]) et en
/// fait ses filtres ; la période et le chantier de l'onglet restent ceux d'en tête.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Aller {
    pub(crate) vue: VueCompta,
    /// Un mois « AAAA-MM ».
    pub(crate) mois: Option<String>,
    pub(crate) section: Option<String>,
    /// Un fournisseur ou un client, nommé.
    pub(crate) tiers: Option<String>,
    /// « a_recevoir », « en_retard », « reglees », « brouillons », « a_verifier » : ce que la liste sait filtrer.
    pub(crate) etat: Option<&'static str>,
}

/// Ouvre une vue avec ce filtre.
pub(crate) fn aller(ctx: &egui::Context, a: Aller) {
    let mut c = choix(ctx);
    c.vue = a.vue;
    choisir(ctx, c);
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(ALLER), a));
}

/// Le filtre d'un « aller à » qui attend sa liste, une seule fois.
pub(crate) fn prendre_l_aller(ctx: &egui::Context, vue: VueCompta) -> Option<Aller> {
    let a = ctx.data(|d| d.get_temp::<Aller>(egui::Id::unique(ALLER)))?;
    if a.vue != vue {
        return None;
    }
    ctx.data_mut(|d| d.remove::<Aller>(egui::Id::unique(ALLER)));
    Some(a)
}

/// Ce que reçoit chaque vue : la lecture, son registre, les filtres d'en tête, le jour (« AAAA-MM-JJ »), et si la
/// fenêtre est étroite (moins de 900 points : les cartes, comme EG au téléphone).
pub(crate) struct Contexte<'a> {
    pub(crate) lecture: &'a Lecture,
    pub(crate) registre: &'a Registre,
    pub(crate) filtres: Filtres,
    pub(crate) aujourdhui: &'a str,
    pub(crate) etroite: bool,
}

/// La place du point ambre devant l'heure d'une lecture qui date (celle du tableau de bord : 18 points, son texte
/// compris).
const POINT: f32 = 14.0;

/// Une demande de lecture au chef au plus toutes les deux minutes tant que l'onglet se montre : le chef décide de
/// l'intervalle réel (il garde la dernière lecture et ne relit qu'au-delà de son délai).
const DE_NOUVEAU: f64 = 120.0;

/// La vue de l'onglet pour une scène de l'essai du thème ; la vitrine, jamais Eve.
pub(crate) fn preparer_pour_l_essai(ctx: &egui::Context, vue: Option<VueCompta>) {
    if let Some(v) = vue {
        choisir(ctx, Choix { vue: v, filtres: Filtres::default() });
    }
}

impl Fenetre {
    /// La lecture que montre l'onglet : celle de la vitrine, si l'essai du thème en impose une ; sinon celle du centre.
    fn lecture_de_la_compta(&self) -> Option<std::sync::Arc<Lecture>> {
        compta::imposee().or_else(|| self.centre.registre())
    }

    /// Demande la lecture au chef (le geste `compta_lire`), au plus toutes les deux minutes.
    fn lire_la_compta(&self, ctx: &egui::Context, forcer: bool) {
        let id = egui::Id::unique(DEMANDEE);
        let maintenant = ctx.input(|i| i.time);
        let avant = ctx.data(|d| d.get_temp::<f64>(id));
        if forcer || avant.is_none_or(|a| maintenant - a > DE_NOUVEAU) {
            ctx.data_mut(|d| d.insert_temp(id, maintenant));
            let valeur = if forcer { Value::Bool(true) } else { Value::Null };
            self.geste("compta_lire", valeur);
        }
    }

    /// La page de l'onglet « Comptabilité », sous la rangée du choix de la vue du tableau de bord : l'entreprise seule
    /// (dans la vue de la famille, un mot qui y mène) ; ses états (pas reliée, la première lecture, une lecture
    /// manquée) ; puis la rangée des vues et des filtres, et la vue.
    pub(super) fn page_de_la_compta(&mut self, ui: &mut egui::Ui, vue: Vue, liaison: Liaison, aujourdhui: &str) {
        if vue == Vue::Famille {
            let texte = "La comptabilité est celle de l'entreprise, lue dans Constructo AI : choisis « Entreprise » ou \
                         « Tout » pour la voir.";
            if etat_vide(
                ui,
                '\u{E8C7}',
                erp::BLEU,
                "La comptabilité montre l'entreprise",
                texte,
                Some("Voir l'entreprise"),
            ) {
                self.choisir_la_vue(ui.ctx(), Vue::Entreprise);
            }
            return;
        }
        let imposee = compta::imposee().is_some();
        if liaison == Liaison::Reliee && !imposee {
            self.lire_la_compta(ui.ctx(), false);
        }
        let lecture = self.lecture_de_la_compta();
        let pas_reliee = matches!(liaison, Liaison::NonReliee | Liaison::EnCours | Liaison::Absente) && !imposee;
        let Some(lecture) = lecture.filter(|l| l.registre.is_some()) else {
            if pas_reliee {
                self.invitation(ui, liaison);
            } else {
                let erreur = self.lecture_de_la_compta().map(|l| l.erreur.clone()).filter(|e| !e.trim().is_empty());
                match erreur {
                    // La carte de l'onglet, pas celle de l'entreprise (`carte_d_erreur`) : son titre dirait « Constructo
                    // AI ne répond pas » quand il répond sans avoir les outils de la lecture, et son « Réessayer »
                    // relirait l'entreprise, pas la comptabilité.
                    Some(raison) => {
                        let titre = "La comptabilité n'a pas pu se lire";
                        if etat_vide(ui, '\u{E8C7}', erp::AMBRE, titre, &raison, Some("Réessayer")) {
                            self.lire_la_compta(ui.ctx(), true);
                        }
                    }
                    None => premiere_lecture_de_la_compta(ui),
                }
            }
            return;
        };
        let Some(registre) = lecture.registre.as_ref() else { return };
        let mut c = choix(ui.ctx());
        // Un chantier ou une année d'en tête que la lecture n'a plus (une nouvelle lecture, une autre entreprise) :
        // retour à « Tous les chantiers », à « Toute la période ». Sans cela, toutes les vues se vidaient en silence,
        // sous un bouton qui disait l'id du chantier (« Projet 135 » : la QA5 du lot 13.2).
        let proposes = choix_du_registre(ui.ctx(), registre, &lecture);
        let permis = filtres_permis(c.filtres, &proposes);
        if permis != c.filtres {
            c.filtres = permis;
            // Un registre tronqué (les plus anciennes y manquent) ne dit pas qu'un chantier n'existe plus : les vues
            // montrent ce qui se lit, et le choix gardé revient avec la lecture entière (la QA5, passe 2 : il se
            // perdait). Une lecture entière, elle, le dit.
            if !registre.tronque {
                choisir(ui.ctx(), c);
            }
        }
        let m = Maintenant::du_poste();
        if let Some(neuf) = self.rangee_de_la_compta(ui, &c, registre, &lecture, liaison, &m) {
            c = neuf;
            choisir(ui.ctx(), c);
        }
        ui.add_space(14.0);
        // Ce que la lecture n'a pas tout dit : une lecture manquée (le registre d'avant reste, et de quand il date),
        // une lecture qui date, un registre tronqué, une base ancienne sans certaines colonnes. Pendant une lecture,
        // « Relire » n'y fait rien : celle qui se fait y répond.
        let (_, quand) = lue_dite(&lecture, &m);
        let (_, relire) = relire_dit(liaison, imposee, &lecture.etat);
        if lecture.etat == crate::entreprise::ERREUR && !lecture.erreur.trim().is_empty() {
            let datent =
                quand.as_ref().map_or(String::new(), |(_, q)| format!(" Ces chiffres datent {}.", de_quand(q)));
            let texte =
                format!("La dernière lecture de la comptabilité n'a pas abouti : {}{datent}", lecture.erreur.trim());
            if super::tableau::bandeau(ui, true, &texte, ("Réessayer", super::theme::pictos::RELIRE)) {
                self.lire_la_compta(ui.ctx(), true);
            }
            ui.add_space(theme::ECART);
        } else if let Some(q) = perimee(&lecture, quand.as_ref(), imposee) {
            let texte = format!("Ces chiffres datent {} : la comptabilité n'a pas été relue depuis.", de_quand(q));
            if super::tableau::bandeau(ui, true, &texte, ("Relire", super::theme::pictos::RELIRE)) && relire {
                self.lire_la_compta(ui.ctx(), true);
            }
            ui.add_space(theme::ECART);
        }
        if registre.tronque {
            let texte = texte_du_registre_tronque(registre);
            if super::tableau::bandeau(ui, false, &texte, ("Relire", super::theme::pictos::RELIRE)) && relire {
                self.lire_la_compta(ui.ctx(), true);
            }
            ui.add_space(theme::ECART);
        }
        let contexte = Contexte {
            lecture: &lecture,
            registre,
            filtres: c.filtres,
            aujourdhui,
            etroite: ui.available_width() < 900.0,
        };
        match c.vue {
            VueCompta::Bord => bord::page(self, ui, &contexte),
            VueCompta::Fournisseurs => listes::page(self, ui, &contexte, Cote::Fournisseur),
            VueCompta::Clients => listes::page(self, ui, &contexte, Cote::Client),
            VueCompta::ParMois | VueCompta::ParFournisseur | VueCompta::ParChantier => {
                sommaires::page(self, ui, &contexte, c.vue)
            }
            VueCompta::Historique => historique::page(self, ui, &contexte),
        }
    }

    /// La rangée de l'onglet : les vues en segments (un bouton à son nom et le menu des vues quand ils ne tiennent pas
    /// dans la largeur, comme le choix du tableau du planning) ; la période et le chantier (un bouton à leur nom, et
    /// leur menu) ; à droite, l'heure de la lecture et le bouton qui la relit. Tout sur une rangée quand tout y tient,
    /// sinon les filtres sur une seconde. Les choix de la période et du chantier se font une fois par lecture (20 000
    /// factures : pas à chaque image). Rend le choix changé.
    fn rangee_de_la_compta(
        &mut self,
        ui: &mut egui::Ui,
        c: &Choix,
        registre: &Registre,
        lecture: &Lecture,
        liaison: Liaison,
        m: &Maintenant,
    ) -> Option<Choix> {
        let mut neuf = None;
        let largeur = ui.available_width();
        let p = ui.painter().clone();
        let proposes = choix_du_registre(ui.ctx(), registre, lecture);
        let (projets, sans, annees) = &*proposes;
        // Ce qui s'y montre, et la place que chaque morceau prend.
        let nom_du_chantier = match c.filtres.chantier {
            Chantier::Tous => "Tous les chantiers".to_string(),
            Chantier::Aucun => "Sans chantier".to_string(),
            Chantier::Projet(id) => projets.iter().find(|p| p.0 == id).map_or(format!("Projet {id}"), |p| p.1.clone()),
        };
        let nom_du_chantier = ecourte(&nom_du_chantier, 40);
        let (relire, relire_actif) = relire_dit(liaison, compta::imposee().is_some(), &lecture.etat);
        let (lue, quand) = lue_dite(lecture, m);
        // Une lecture qui date d'une demi-heure et plus : un point ambre devant son heure, comme les chiffres de
        // l'entreprise.
        let ambre = perimee(lecture, quand.as_ref(), compta::imposee().is_some()).is_some();
        let bouton =
            |code: char, texte: &str| theme::taille_du_bouton_erp(&p, Some(code), texte, BoutonErp::Secondaire).x;
        let k = VUES.iter().position(|(v, _, _)| *v == c.vue).unwrap_or(0);
        let l_segments = largeur_des_vues(&p);
        let en_segments = l_segments <= largeur;
        let l_vues = if en_segments { l_segments } else { bouton('\u{E70D}', VUES[k].1) };
        let l_filtres = bouton('\u{E787}', &c.filtres.periode.libelle()) + 8.0 + bouton('\u{E80F}', &nom_du_chantier);
        let police_de_lue = egui::FontId::proportional(13.0);
        let l_point = if ambre { POINT } else { 0.0 };
        let l_lue = if lue.is_empty() {
            0.0
        } else {
            p.layout_no_wrap(lue.clone(), police_de_lue, erp::TEXTE_2).size().x + l_point
        };
        let l_droite = l_lue + if relire { bouton(super::theme::pictos::RELIRE, "Relire") + 10.0 } else { 0.0 };
        let une_rangee = l_vues + 16.0 + l_filtres + 16.0 + l_droite <= largeur;
        let droite_en_haut = une_rangee || l_vues + 16.0 + l_droite <= largeur;
        // Les vues.
        let (haut, _) = ui.allocate_exact_size(vec2(largeur, 40.0), Sense::hover());
        let mut rangee = ui.new_child(UiBuilder::new().max_rect(haut).layout(Layout::left_to_right(Align::Center)));
        if let Some(vue) = choix_de_la_vue(&mut rangee, k, en_segments) {
            neuf = Some(Choix { vue, ..*c });
        }
        let (mut bas, place_des_filtres) = if une_rangee {
            rangee.add_space(16.0);
            (rangee, haut)
        } else {
            ui.add_space(10.0);
            let (place, _) = ui.allocate_exact_size(vec2(largeur, 38.0), Sense::hover());
            (ui.new_child(UiBuilder::new().max_rect(place).layout(Layout::left_to_right(Align::Center))), place)
        };
        // La période.
        let mut periodes = vec![Periode::Tout, Periode::DouzeMois];
        periodes.extend(annees.iter().copied().map(Periode::Annee));
        let rep = theme::bouton_erp_nomme(
            &mut bas,
            Some(egui::Id::unique("compta-periode")),
            Some('\u{E787}'),
            &c.filtres.periode.libelle(),
            BoutonErp::Secondaire,
        );
        theme::menu_blanc(&rep).show(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for (j, p) in periodes.iter().enumerate() {
                let marque = if *p == c.filtres.periode { '\u{E73E}' } else { '\u{E787}' };
                let id = egui::Id::unique(("compta-periode-ligne", j));
                if theme::ligne_du_menu(ui, marque, &p.libelle(), false, id).clicked() {
                    neuf = Some(Choix { filtres: Filtres { periode: *p, ..c.filtres }, ..neuf.unwrap_or(*c) });
                }
            }
        });
        bas.add_space(8.0);
        // Le chantier.
        let rep = theme::bouton_erp_nomme(
            &mut bas,
            Some(egui::Id::unique("compta-chantier")),
            Some('\u{E80F}'),
            &nom_du_chantier,
            BoutonErp::Secondaire,
        );
        let mut choix_du_chantier: Vec<(Chantier, &str)> = vec![(Chantier::Tous, "Tous les chantiers")];
        choix_du_chantier.extend(projets.iter().map(|(id, nom)| (Chantier::Projet(*id), nom.as_str())));
        if *sans {
            choix_du_chantier.push((Chantier::Aucun, "Sans chantier"));
        }
        theme::menu_blanc(&rep).show(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                for (j, (ch, nom)) in choix_du_chantier.iter().enumerate() {
                    let marque = if *ch == c.filtres.chantier { '\u{E73E}' } else { '\u{E80F}' };
                    let id = egui::Id::unique(("compta-chantier-ligne", j));
                    if theme::ligne_du_menu(ui, marque, &ecourte(nom, 48), false, id).clicked() {
                        let base = neuf.unwrap_or(*c);
                        neuf = Some(Choix { filtres: Filtres { chantier: *ch, ..base.filtres }, ..base });
                    }
                }
            });
        });
        // À droite : l'heure de la lecture, et le bouton qui la relit (reliée seulement) — sur la rangée des vues
        // quand elle y tient.
        let place_de_droite = if droite_en_haut { haut } else { place_des_filtres };
        let mut droite =
            ui.new_child(UiBuilder::new().max_rect(place_de_droite).layout(Layout::right_to_left(Align::Center)));
        if relire {
            // Pendant une lecture, éteint (comme celui du tableau de bord) : celle qui se fait y répond.
            let rep = droite.add_enabled_ui(relire_actif, |ui| {
                theme::bouton_erp(ui, Some(super::theme::pictos::RELIRE), "Relire", BoutonErp::Secondaire)
            });
            let rep = rep
                .inner
                .on_hover_text("Relire la comptabilité dans Constructo AI")
                .on_disabled_hover_text("Eve lit la comptabilité de Constructo AI…");
            if relire_actif && rep.clicked() {
                self.lire_la_compta(droite.ctx(), true);
            }
        }
        if !lue.is_empty() {
            droite.label(egui::RichText::new(lue).size(13.0).color(erp::TEXTE_2));
            if ambre {
                let (r, _) = droite.allocate_exact_size(vec2(POINT, 32.0), Sense::hover());
                droite.painter().circle_filled(egui::pos2(r.left() + 6.0, r.center().y), 4.0, erp::AVERTISSEMENT);
            }
        }
        neuf
    }
}

/// Ce que la rangée de l'onglet propose d'après le registre : ses chantiers (et s'il a des factures sans chantier), ses
/// années.
type ChoixDuRegistre = (Vec<(i64, String)>, bool, Vec<i32>);

/// Les choix du registre, refaits seulement quand la lecture change (son heure, son nombre de factures) : les chercher
/// parcourt toutes les factures, jusqu'à 20 000, et la rangée se dessine à chaque image.
fn choix_du_registre(ctx: &egui::Context, registre: &Registre, lecture: &Lecture) -> std::sync::Arc<ChoixDuRegistre> {
    let id = egui::Id::unique("compta-choix-du-registre");
    let cle = (lecture.lu_le.clone(), registre.factures.len());
    type EnMemoire = ((String, usize), std::sync::Arc<ChoixDuRegistre>);
    if let Some((avant, choix)) = ctx.data(|d| d.get_temp::<EnMemoire>(id))
        && avant == cle
    {
        return choix;
    }
    let (projets, sans) = compta::chantiers(registre);
    let choix = std::sync::Arc::new((projets, sans, compta::annees(registre)));
    ctx.data_mut(|d| d.insert_temp(id, (cle, choix.clone())));
    choix
}

/// Les filtres d'en tête que la lecture permet encore : un chantier ou une année qu'elle n'a plus (une nouvelle
/// lecture, une autre entreprise) reviennent à « Tous les chantiers », à « Toute la période ». Sans cela, toutes les
/// vues se vidaient en silence, sous un bouton qui disait l'id du chantier (« Projet 135 » : la QA5 du lot 13.2).
fn filtres_permis(f: Filtres, proposes: &ChoixDuRegistre) -> Filtres {
    let chantier = match f.chantier {
        Chantier::Projet(id) if !proposes.0.iter().any(|(p, _)| *p == id) => Chantier::Tous,
        Chantier::Aucun if !proposes.1 => Chantier::Tous,
        c => c,
    };
    let periode = match f.periode {
        Periode::Annee(a) if !proposes.2.contains(&a) => Periode::Tout,
        p => p,
    };
    Filtres { chantier, periode }
}

/// La largeur des vues en segments (celle que `theme::segments_erp` leur donne à 34 points de haut : chaque mot et 28
/// points, la piste et ses écarts).
fn largeur_des_vues(p: &egui::Painter) -> f32 {
    let police = egui::FontId::new(13.0, egui::FontFamily::Name(super::SEMI.into()));
    let mots: f32 = VUES
        .iter()
        .map(|(_, mot, _)| p.layout_no_wrap(mot.to_string(), police.clone(), erp::TEXTE).size().x + 28.0)
        .sum();
    mots + 8.0 + 2.0 * (VUES.len() - 1) as f32
}

/// Le choix de la vue : en segments, ou un bouton au nom de la vue `k` et le menu des vues. Rend la vue choisie.
fn choix_de_la_vue(ui: &mut egui::Ui, k: usize, en_segments: bool) -> Option<VueCompta> {
    if en_segments {
        let segments: Vec<theme::Segment> =
            VUES.iter().map(|(_, mot, aide)| theme::Segment { icone: None, mot, aide }).collect();
        return theme::segments_erp(ui, "vues-de-la-compta", &segments, k, 34.0).map(|j| VUES[j].0);
    }
    let id = Some(egui::Id::unique("compta-menu-des-vues"));
    let rep = theme::bouton_erp_nomme(ui, id, Some('\u{E70D}'), VUES[k].1, BoutonErp::Secondaire);
    let mut choisie = None;
    theme::menu_blanc(&rep).show(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        for (j, (vue, mot, _)) in VUES.iter().enumerate() {
            let marque = if j == k { '\u{E73E}' } else { super::theme::pictos::LISTE };
            if theme::ligne_du_menu(ui, marque, mot, false, egui::Id::unique(("compta-ligne-des-vues", j))).clicked() {
                choisie = Some(*vue);
            }
        }
    });
    choisie
}

/// Un nom coupé à `n` caractères au plus, d'une ellipse.
fn ecourte(nom: &str, n: usize) -> String {
    if nom.chars().count() <= n {
        return nom.to_string();
    }
    let mut court: String = nom.chars().take(n.saturating_sub(1)).collect();
    court.push('…');
    court
}

// ---- Ce que l'onglet dit de sa lecture ------------------------------------------------------------------------------

/// Ce que dit la rangée de l'onglet de l'heure de la lecture, comme l'en-tête du tableau de bord : « Lecture en
/// cours… » pendant une lecture ; sinon « Lue à 8 h 12 », « Lue hier à 17 h 30 », « Lue le 30 oct. à 17 h 30 » (la QA5
/// du lot 13.2 : « Lue à 17 h 30 » taisait le jour) ; rien sans heure lisible. Et son âge en secondes, et quand, à dire
/// après « Ces chiffres datent » ([`de_quand`]).
fn lue_dite(lecture: &Lecture, m: &Maintenant) -> (String, Option<(i64, String)>) {
    let Some((age, local)) = m.age(&lecture.lu_le) else { return (String::new(), None) };
    let q = quand_dit(local, m);
    let texte = if lecture.etat == crate::entreprise::EN_COURS {
        "Lecture en cours…".to_string()
    } else {
        format!("Lue {q}")
    };
    (texte, Some((age, q)))
}

/// La lecture date-t-elle — à jour, d'une demi-heure et plus ([`PERIMES`], comme les chiffres de l'entreprise :
/// l'onglet la redemande toutes les dix minutes quand il se montre) ? Rend quand elle a été faite. Jamais pour la
/// lecture qu'impose l'essai du thème (la vitrine, à son heure).
fn perimee<'a>(lecture: &Lecture, quand: Option<&'a (i64, String)>, imposee: bool) -> Option<&'a str> {
    let (age, q) = quand?;
    (!imposee && lecture.etat == crate::entreprise::A_JOUR && *age >= PERIMES).then_some(q.as_str())
}

/// « Relire » : se montre-t-il, et répond-il ? Il se montre Eve reliée à Constructo AI, jamais sur la vitrine ; il ne
/// répond pas pendant une lecture, comme celui du tableau de bord — celle qui se fait y répond (la QA5 du lot 13.2).
fn relire_dit(liaison: Liaison, imposee: bool, etat: &str) -> (bool, bool) {
    let montre = liaison == Liaison::Reliee && !imposee;
    (montre, montre && etat != crate::entreprise::EN_COURS)
}

/// Le bandeau d'un registre tronqué. Le registre se lit des plus récentes aux plus anciennes (`recentes_d_abord`) : à
/// la borne (`compta::FACTURES_MAX`), ce sont les plus anciennes qui manquent, jamais celles du moment. Les nombres,
/// une espace fine entre les milliers.
fn texte_du_registre_tronque(registre: &Registre) -> String {
    format!(
        "Eve a lu les {} factures les plus récentes sur les {} de Constructo AI : les plus anciennes n'y sont pas.",
        crate::centre::milliers(registre.factures.len() as f64),
        crate::centre::milliers(registre.au_serveur as f64)
    )
}

/// La première lecture de la comptabilité, pas encore là : une roue, et ce qu'Eve fait — la carte de l'onglet, pas
/// celle de l'entreprise (la QA5 du lot 13.2 : elle disait « Entreprise », et promettait une trentaine de secondes).
fn premiere_lecture_de_la_compta(ui: &mut egui::Ui) {
    theme::carte(ui, Some(erp::BLEU.pastel), 0.0, |ui| {
        theme::tete_de_carte(ui, "Comptabilité", '\u{E8C7}', erp::BLEU, false, None);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let (r, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
            super::tableau::roue(ui, r.center(), 7.0, erp::PRIMAIRE);
            ui.label(egui::RichText::new("Eve lit la comptabilité de Constructo AI…").size(14.0).color(erp::TEXTE));
        });
        let aide = "Les factures, puis l'historique, se lisent page par page : quelques secondes, jusqu'à une minute \
                    pour une grande entreprise.";
        ui.add(egui::Label::new(egui::RichText::new(aide).size(12.5).color(erp::TEXTE_2)).wrap().selectable(false));
    });
}

#[cfg(test)]
mod essais {
    use super::*;

    /// Le poste à l'heure de l'Est (−5 h) : 15 h 10 locale, le 2 novembre 2026 (les heures de lecture de ces essais
    /// sont écrites sans fuseau, comme avant la passe 2 : l'heure du poste seule y compte).
    fn maintenant() -> Maintenant {
        let jour = crate::centre::numero_du_jour("2026-11-02").unwrap();
        Maintenant { jour, local: jour * 86_400 + 15 * 3600 + 600, unix: jour * 86_400 + 20 * 3600 + 600 }
    }

    fn lecture(etat: &str, lu_le: &str) -> Lecture {
        Lecture { etat: etat.into(), lu_le: lu_le.into(), ..Lecture::default() }
    }

    /// L'heure de la lecture dit son jour, comme l'en-tête du tableau de bord : aujourd'hui, hier, ou sa date ; pendant
    /// une lecture, qu'elle se fait ; rien sans heure lisible.
    #[test]
    fn l_heure_de_la_lecture_dit_son_jour() {
        let m = maintenant();
        let dite = |etat: &str, lu_le: &str| lue_dite(&lecture(etat, lu_le), &m);
        assert_eq!(dite("a_jour", "2026-11-02T08:12:00").0, "Lue à 8 h 12");
        assert_eq!(dite("erreur", "2026-11-01T17:30:00").0, "Lue hier à 17 h 30");
        assert_eq!(dite("a_jour", "2026-10-30T17:30:00").0, "Lue le 30 oct. à 17 h 30");
        assert_eq!(dite("en_cours", "2026-10-30T17:30:00").0, "Lecture en cours…");
        assert_eq!(dite("a_jour", ""), (String::new(), None));
        let (_, quand) = dite("a_jour", "2026-11-01T17:30:00");
        assert_eq!(quand, Some((21 * 3600 + 40 * 60, "hier à 17 h 30".to_string())));
        assert_eq!(de_quand(&quand.unwrap().1), "d'hier à 17 h 30");
    }

    /// Une lecture à jour qui date d'une demi-heure et plus se dit ; pas une plus récente, ni une lecture qui manque
    /// (son bandeau dit déjà de quand datent les factures) ou qui se fait, ni la vitrine.
    #[test]
    fn une_lecture_qui_date_se_dit() {
        let m = maintenant();
        let dit = |etat: &str, lu_le: &str, imposee: bool| {
            let l = lecture(etat, lu_le);
            let (_, quand) = lue_dite(&l, &m);
            perimee(&l, quand.as_ref(), imposee).map(str::to_string)
        };
        assert_eq!(dit("a_jour", "2026-11-02T14:40:00", false), Some("à 14 h 40".to_string()), "trente minutes");
        assert_eq!(dit("a_jour", "2026-11-02T14:40:01", false), None, "une seconde de moins");
        assert_eq!(dit("a_jour", "2026-11-01T17:30:00", false), Some("hier à 17 h 30".to_string()));
        assert_eq!(dit("a_jour", "2026-11-01T17:30:00", true), None, "la vitrine");
        assert_eq!(dit("erreur", "2026-11-01T17:30:00", false), None);
        assert_eq!(dit("en_cours", "2026-11-01T17:30:00", false), None);
    }

    /// La QA5 (passe 2) : au changement d'heure, une lecture d'un quart d'heure « datait » de 1 h 15 — `lu_le` était
    /// écrit sans fuseau. Il l'est avec son décalage (`entreprise::maintenant_iso`) : l'âge se compte juste.
    #[test]
    fn au_changement_d_heure_l_age_d_une_lecture_est_juste() {
        // Le 8 mars 2026, 3 h 10 à l'heure avancée (−4 h) ; lue à 1 h 55 à l'heure normale (−5 h), un quart d'heure
        // plus tôt : 2 h 55 à l'heure d'aujourd'hui.
        let jour = crate::centre::numero_du_jour("2026-03-08").unwrap();
        let m = Maintenant { jour, local: jour * 86_400 + 3 * 3600 + 600, unix: jour * 86_400 + 7 * 3600 + 600 };
        let l = lecture("a_jour", "2026-03-08T01:55:00-05:00");
        let (texte, quand) = lue_dite(&l, &m);
        assert_eq!((texte.as_str(), quand.as_ref().map(|q| q.0)), ("Lue à 2 h 55", Some(15 * 60)));
        assert_eq!(perimee(&l, quand.as_ref(), false), None);
        // Sans fuseau (un registre écrit avant), l'heure du poste seule : elle « daterait » de 1 h 15.
        let (_, ancienne) = lue_dite(&lecture("a_jour", "2026-03-08T01:55:00"), &m);
        assert_eq!(ancienne.map(|q| q.0), Some(75 * 60));
    }

    /// « Relire » se montre reliée seulement, et ne répond pas pendant une lecture.
    #[test]
    fn relire_ne_repond_pas_pendant_une_lecture() {
        assert_eq!(relire_dit(Liaison::Reliee, false, "a_jour"), (true, true));
        assert_eq!(relire_dit(Liaison::Reliee, false, "erreur"), (true, true));
        assert_eq!(relire_dit(Liaison::Reliee, false, "en_cours"), (true, false));
        assert_eq!(relire_dit(Liaison::Reliee, true, "a_jour"), (false, false), "la vitrine");
        assert_eq!(relire_dit(Liaison::NonReliee, false, "a_jour"), (false, false));
    }

    /// Un chantier, « Sans chantier » ou une année que la lecture n'a plus reviennent à « Tous » (la QA5, passe 2,
    /// mutation : sous les mutants 701, 702 et 703, ils restaient choisis, et aucun essai ne le voyait).
    #[test]
    fn un_choix_que_la_lecture_n_a_plus_revient_a_tous() {
        let proposes: ChoixDuRegistre = (vec![(7, "Bureau".to_string())], false, vec![2026]);
        let f = |chantier, periode| Filtres { periode, chantier };
        let tous_12 = f(Chantier::Tous, Periode::DouzeMois);
        assert_eq!(filtres_permis(f(Chantier::Projet(135), Periode::DouzeMois), &proposes), tous_12);
        assert_eq!(filtres_permis(f(Chantier::Aucun, Periode::Tout), &proposes), f(Chantier::Tous, Periode::Tout));
        assert_eq!(
            filtres_permis(f(Chantier::Tous, Periode::Annee(2024)), &proposes),
            f(Chantier::Tous, Periode::Tout)
        );
        let gardes = f(Chantier::Projet(7), Periode::Annee(2026));
        assert_eq!(filtres_permis(gardes, &proposes), gardes);
    }

    /// Un registre tronqué dit que ce sont les plus anciennes qui manquent.
    #[test]
    fn un_registre_tronque_dit_que_les_anciennes_manquent() {
        let r = Registre {
            factures: vec![compta::Facture::default(); 3],
            au_serveur: 30_000,
            tronque: true,
            ..Registre::default()
        };
        assert_eq!(
            texte_du_registre_tronque(&r),
            "Eve a lu les 3 factures les plus récentes sur les 30\u{202f}000 de Constructo AI : les plus anciennes n'y \
             sont pas."
        );
    }
}
