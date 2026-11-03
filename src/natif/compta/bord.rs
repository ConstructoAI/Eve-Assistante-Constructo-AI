//! Le tableau de bord de l'onglet « Comptabilité », celui d'Entrepreneur Général (`comptabilite.html`, son tableau de
//! bord : « plein de beaux graphismes, tableaux, pour bien voir de façon intuitive », la demande) : en tête, les
//! indicateurs des dépenses puis ceux des revenus, en tuiles ; dessous, ses graphiques en cartes — les dépenses et les
//! revenus par mois et leur moyenne, l'état de la saisie, par chantier, par corps de métier, les principaux
//! fournisseurs, les taxes payées par trimestre — et l'estimation de la TPS et de la TVQ à remettre. Un clic mène à ses
//! factures : une tuile, la colonne d'un mois, une section, un fournisseur ouvrent la liste filtrée (`super::aller`) ;
//! un chantier devient celui d'en tête (`super::choisir`). Étroite, une seule colonne et les tuiles par deux.
//!
//! Le calcul (`crate::compta::bord`) et ses graphiques se gardent dans la mémoire d'egui tant que les filtres, la
//! lecture (`lu_le`, le nombre de factures) et le jour sont les mêmes : rien ne se refait à chaque image, même pour
//! 20 000 factures.

use std::sync::Arc;

use eframe::egui::{self, Align, Color32, FontFamily, FontId, Layout, Rect, RichText, Sense, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

use super::super::graphiques::{self, Forme, Graphique, Precisions, SERIES, Serie};
use super::super::planning::etat_vide;
use super::super::theme::{self, LigneDeMenu, erp, pictos};
use super::super::{Fenetre, GRAS, ICONES, SEMI};
use super::{Aller, Choix, Contexte, VueCompta};
use crate::compta::bord::{self as calcul, Bord, EtatDuTrimestre, Indicateur, Sorte, Vers};
use crate::compta::{self, Chantier, Filtres, Periode, argent};

/// Les couleurs des séries : les dépenses en bleu, les revenus en aqua (le vert d'eau d'EG ; les trois premiers rangs
/// de la palette de la compétence se distinguent deux à deux, même pour qui voit mal les couleurs) ; la TPS en bleu et
/// la TVQ en orange, comme EG ; une dépense sans section, l'ardoise.
const DEPENSES: Color32 = SERIES[0];
const REVENUS: Color32 = SERIES[2];
const TPS: Color32 = SERIES[0];
const TVQ: Color32 = SERIES[1];
const SANS_SECTION: Color32 = erp::GRIS.encre;
/// La fiabilité de la saisie : un état, dans les couleurs d'état (sûre, à vérifier), toujours nommé à côté.
const SURE: Color32 = erp::SUCCES;
const A_VERIFIER: Color32 = erp::AVERTISSEMENT;
const NON_JUGEE: Color32 = erp::GRIS.pastel;
/// Les sections que l'anneau colore, les plus grosses ; les autres y sont réunies, en gris.
const SECTIONS_COLOREES: usize = 5;

// ---- Ce qui se garde d'une image à l'autre --------------------------------------------------------------------------

/// Un graphique qu'on touche, et ce qu'un clic choisit sur chacune de ses catégories (rien : elle ne mène nulle part —
/// le graphique la tient alors pour une des « autres », sans la main).
struct Touchable<T> {
    graphique: Graphique,
    choix: Vec<Option<T>>,
}

impl<T: Clone> Touchable<T> {
    /// Le graphique, ses catégories sans choix marquées « autres ».
    fn de(mut graphique: Graphique, choix: Vec<Option<T>>) -> Touchable<T> {
        graphique.precisions.autres = (0..choix.len()).filter(|k| choix[*k].is_none()).collect();
        Touchable { graphique, choix }
    }

    /// Le graphique, dessiné ; ce que choisit le clic qu'il a reçu.
    fn toucher(&self, ui: &mut egui::Ui, cle: &str) -> Option<(T, Option<usize>)> {
        let t = graphiques::graphique_touche(ui, &self.graphique, cle)?;
        self.choix.get(t.categorie).cloned().flatten().map(|c| (c, t.serie))
    }
}

/// Ce que montre le tableau de bord, préparé une fois pour des filtres, une lecture et un jour : le calcul, les
/// indicateurs, les graphiques, et ce qu'un clic choisit sur chacun.
struct Vue {
    bord: Bord,
    filtres: Filtres,
    depenses: Vec<Indicateur>,
    revenus: Vec<Indicateur>,
    /// Le graphique par mois : la clé de chaque mois (« 2026-09 »).
    mois: Option<Touchable<String>>,
    /// Par chantier : le chantier que choisit chaque rangée (aucun pour celle des autres).
    chantiers: Option<Touchable<Chantier>>,
    /// Par corps de métier : l'anneau et la liste, la section de chaque part et de chaque rangée (aucune pour les autres
    /// sections réunies, ni sans section).
    sections: Option<(Option<Touchable<String>>, Touchable<String>)>,
    /// Les principaux fournisseurs : le nom de chaque rangée (aucun pour les autres, ni pour un fournisseur sans nom).
    fournisseurs: Option<Touchable<String>>,
    taxes: Option<Graphique>,
    /// L'estimation des taxes à remettre : son graphique et son tableau (une carte « tableau », en JSON).
    declaration: Option<(Graphique, String)>,
    /// La période et le chantier, dits (« 12 derniers mois, tous les chantiers ») : pour une demande à Eve.
    filtres_dits: String,
}

/// La clé de la mémoire : les filtres, l'heure de la lecture, le nombre de factures, le jour.
type Cle = (Filtres, String, usize, String);

#[derive(Clone)]
struct EnMemoire {
    cle: Cle,
    vue: Arc<Vue>,
}

/// La vue préparée, celle de la mémoire si rien n'a changé.
fn vue_en_memoire(ctx: &egui::Context, c: &Contexte<'_>) -> Arc<Vue> {
    let id = egui::Id::unique("compta-bord-en-memoire");
    let cle: Cle = (c.filtres, c.lecture.lu_le.clone(), c.registre.factures.len(), c.aujourdhui.to_string());
    if let Some(m) = ctx.data(|d| d.get_temp::<EnMemoire>(id))
        && m.cle == cle
    {
        return m.vue;
    }
    let vue = Arc::new(preparer(c));
    ctx.data_mut(|d| d.insert_temp(id, EnMemoire { cle, vue: vue.clone() }));
    vue
}

/// Des montants en centimes, dits au cent (les graphiques ne les arrondissent jamais), de ces couleurs.
fn centimes(couleurs: Vec<Color32>) -> Precisions {
    Precisions { centimes: true, couleurs, ..Precisions::default() }
}

/// Une série de montants : des centimes, passés en flottant pour les seules longueurs à l'écran (les textes se disent
/// d'après eux, au cent : `Precisions::centimes`).
fn serie(nom: &str, valeurs: impl Iterator<Item = compta::Cents>) -> Serie {
    Serie { nom: nom.into(), valeurs: valeurs.map(|v| Some(v as f64)).collect() }
}

/// Un graphique fait ici, en dollars.
fn graphique(forme: Forme, etiquettes: Vec<String>, series: Vec<Serie>, precisions: Precisions) -> Graphique {
    Graphique { forme, etiquettes, series, unite: "$".into(), souligne: None, autres: false, precisions }
}

/// Le calcul et ses graphiques.
fn preparer(c: &Contexte<'_>) -> Vue {
    let b = calcul::calculer(c.registre, c.filtres, c.aujourdhui);
    let avec_revenus = b.revenus.n > 0;
    // Par mois : les dépenses, et les revenus à côté s'il y en a ; la moyenne des dépenses ; l'axe en bref (l'année à
    // la place de janvier), la bulle en entier.
    let mois = (!b.mois.is_empty()).then(|| {
        let mut series = vec![serie("Dépenses", b.mois.iter().map(|m| m.depenses))];
        let mut couleurs = vec![DEPENSES];
        if avec_revenus {
            series.push(serie("Revenus", b.mois.iter().map(|m| m.revenus)));
            couleurs.push(REVENUS);
        }
        let nom = if avec_revenus { "Moyenne des dépenses par mois" } else { "Moyenne par mois" };
        let repere = (b.moyenne > 0).then(|| (b.moyenne as f64, format!("{nom} : {}", compta::compact(b.moyenne))));
        let etiquettes = b
            .mois
            .iter()
            .map(
                |m| if m.mois == 1 { m.annee.to_string() } else { compta::MOIS_COURTS[usize::from(m.mois) - 1].into() },
            )
            .collect();
        let titres = b.mois.iter().map(calcul::Mois::nom).collect();
        let precisions = Precisions { repere, titres, ..centimes(couleurs) };
        let choix = b.mois.iter().map(|m| Some(m.cle())).collect();
        Touchable::de(graphique(Forme::Colonnes, etiquettes, series, precisions), choix)
    });
    // Par chantier (tous les chantiers seulement) : du plus coûteux au moins coûteux, ceux d'après réunis (sans clic),
    // puis les factures sans chantier.
    let chantiers = (c.filtres.chantier == Chantier::Tous && (!b.chantiers.is_empty() || b.sans_chantier.is_some()))
        .then(|| {
            let mut lignes: Vec<(&calcul::LigneDeChantier, Option<Chantier>)> =
                b.chantiers.iter().map(|l| (l, Some(l.chantier))).collect();
            if let Some((_, l)) = &b.autres_chantiers {
                lignes.push((l, None));
            }
            if let Some(l) = &b.sans_chantier {
                lignes.push((l, Some(Chantier::Aucun)));
            }
            let mut series = vec![serie("Dépenses", lignes.iter().map(|(l, _)| l.depenses))];
            let mut couleurs = vec![DEPENSES];
            if avec_revenus {
                series.push(serie("Revenus", lignes.iter().map(|(l, _)| l.revenus)));
                couleurs.push(REVENUS);
            }
            let etiquettes = lignes.iter().map(|(l, _)| l.nom.clone()).collect();
            let choix = lignes.iter().map(|(_, ch)| *ch).collect();
            Touchable::de(graphique(Forme::Horizontales, etiquettes, series, centimes(couleurs)), choix)
        });
    // Par corps de métier : l'anneau (les cinq plus grosses sections ; les autres réunies, en gris ; les dépenses sans
    // section, à part) et la liste de toutes, chacune de la couleur de sa part. Une section mène à ses factures ; les
    // autres réunies et « Sans section », nulle part.
    let sections = (b.depenses.n > 0).then(|| {
        let couleur = |i: usize, total: compta::Cents| {
            if i < SECTIONS_COLOREES && total > 0 { SERIES[i] } else { graphiques_gris() }
        };
        let mut etiquettes: Vec<String> = b.sections.iter().map(|g| g.nom.clone()).collect();
        let mut valeurs: Vec<compta::Cents> = b.sections.iter().map(|g| g.total).collect();
        let mut couleurs: Vec<Color32> = b.sections.iter().enumerate().map(|(i, g)| couleur(i, g.total)).collect();
        let mut choix: Vec<Option<String>> = b.sections.iter().map(|g| Some(g.nom.clone())).collect();
        if let Some(g) = &b.sans_section {
            etiquettes.push("Sans section".into());
            valeurs.push(g.total);
            couleurs.push(SANS_SECTION);
            choix.push(None);
        }
        let series = vec![serie("Dépenses", valeurs.into_iter())];
        let liste = Touchable::de(graphique(Forme::Horizontales, etiquettes, series, centimes(couleurs)), choix);
        // L'anneau : seulement des parts positives.
        let mut parts: Vec<(String, compta::Cents, Color32, Option<String>)> = Vec::new();
        let positives: Vec<&calcul::Groupe> = b.sections.iter().filter(|g| g.total > 0).collect();
        for (i, g) in positives.iter().enumerate().take(SECTIONS_COLOREES) {
            parts.push((g.nom.clone(), g.total, SERIES[i], Some(g.nom.clone())));
        }
        if positives.len() > SECTIONS_COLOREES {
            let reste = &positives[SECTIONS_COLOREES..];
            let nom = if reste.len() == 1 {
                "L'autre section".to_string()
            } else {
                format!("Les {} autres sections", reste.len())
            };
            parts.push((nom, reste.iter().map(|g| g.total).sum(), graphiques_gris(), None));
        }
        if let Some(g) = b.sans_section.as_ref().filter(|g| g.total > 0) {
            parts.push(("Sans section".into(), g.total, SANS_SECTION, None));
        }
        let anneau = (!parts.is_empty()).then(|| {
            let etiquettes = parts.iter().map(|p| p.0.clone()).collect();
            let teintes = parts.iter().map(|p| p.2).collect();
            // Une section négative (une note de crédit plus grosse que ses factures) n'a pas de part : la somme du
            // centre n'est alors pas le total des dépenses, et le centre le dit (la QA5, passe 1 : « 11,5 k$ au
            // total » pour 10 347,75 $ de dépenses).
            let negatives = b.sections.iter().chain(b.sans_section.iter()).any(|g| g.total < 0);
            let mot_du_centre = negatives.then(|| "parts positives".to_string());
            let precisions = Precisions { sans_legende: true, mot_du_centre, ..centimes(teintes) };
            let series = vec![serie("Dépenses", parts.iter().map(|p| p.1))];
            Touchable::de(
                graphique(Forme::Anneau, etiquettes, series, precisions),
                parts.iter().map(|p| p.3.clone()).collect(),
            )
        });
        (anneau, liste)
    });
    // Les principaux fournisseurs : les dix premiers, les autres réunis (sans clic, comme un fournisseur sans nom).
    let fournisseurs = (b.depenses.n > 0).then(|| {
        let mut etiquettes: Vec<String> = b
            .fournisseurs
            .iter()
            .map(|g| if g.nom.is_empty() { "Fournisseur sans nom".to_string() } else { g.nom.clone() })
            .collect();
        let mut valeurs: Vec<compta::Cents> = b.fournisseurs.iter().map(|g| g.total).collect();
        let mut choix: Vec<Option<String>> =
            b.fournisseurs.iter().map(|g| (!g.nom.is_empty()).then(|| g.nom.clone())).collect();
        if let Some((_, g)) = &b.autres_fournisseurs {
            etiquettes.push(g.nom.clone());
            valeurs.push(g.total);
            choix.push(None);
        }
        let series = vec![serie("Dépenses", valeurs.into_iter())];
        Touchable::de(graphique(Forme::Horizontales, etiquettes, series, centimes(Vec::new())), choix)
    });
    // Les taxes payées par trimestre : la TPS et la TVQ côte à côte.
    let trimestres = &b.trimestres;
    let etiquettes = || trimestres.iter().map(calcul::Trimestre::court).collect::<Vec<String>>();
    let titres = || {
        trimestres
            .iter()
            .map(|t| {
                let etat = match t.etat {
                    EtatDuTrimestre::Clos => "",
                    EtatDuTrimestre::EnCours => " · en cours",
                    EtatDuTrimestre::AVenir => " · à venir",
                };
                format!("{} · {}{etat}", t.court(), t.mois())
            })
            .collect::<Vec<String>>()
    };
    let (tps, tvq) = (b.taxes.0.as_str(), b.taxes.1.as_str());
    // Une seule taxe nommée (la TVH, une entreprise à la TPS seule, une « Sales Tax ») : une seule série, jamais une
    // seconde vide ; sous son nom, la somme des deux cases, comme les tuiles (`calcul::deux_taxes`).
    let une_taxe = tvq.is_empty();
    let taxes = (b.depenses.n > 0 && !trimestres.is_empty()).then(|| {
        let (series, couleurs) = if une_taxe {
            (vec![serie(tps, trimestres.iter().map(|t| t.tps_payee + t.tvq_payee))], vec![TPS])
        } else {
            let series = vec![
                serie(tps, trimestres.iter().map(|t| t.tps_payee)),
                serie(tvq, trimestres.iter().map(|t| t.tvq_payee)),
            ];
            (series, vec![TPS, TVQ])
        };
        graphique(Forme::Colonnes, etiquettes(), series, Precisions { titres: titres(), ..centimes(couleurs) })
    });
    // L'estimation de ce qui est à remettre : avec des revenus, seulement si les taxes se récupèrent (la TPS, la TVH,
    // la TVQ). Une seule taxe : une seule série, sa nette (la QA5, passe 2 : une série nommée « nette » et un titre
    // « TPS et  à remettre »).
    let declaration = (b.taxes_recuperables && avec_revenus && !trimestres.is_empty()).then(|| {
        let (series, couleurs) = if une_taxe {
            let nette = trimestres.iter().map(|t| t.tps_nette() + t.tvq_nette());
            (vec![serie(&format!("{tps} nette"), nette)], vec![TPS])
        } else {
            let series = vec![
                serie(&format!("{tps} nette"), trimestres.iter().map(calcul::Trimestre::tps_nette)),
                serie(&format!("{tvq} nette"), trimestres.iter().map(calcul::Trimestre::tvq_nette)),
            ];
            (series, vec![TPS, TVQ])
        };
        let g = graphique(Forme::Colonnes, etiquettes(), series, Precisions { titres: titres(), ..centimes(couleurs) });
        (g, tableau_de_la_declaration(&b))
    });
    let (projets, _) = compta::chantiers(c.registre);
    let chantier = match c.filtres.chantier {
        Chantier::Tous => "tous les chantiers".to_string(),
        Chantier::Aucun => "les factures sans chantier".to_string(),
        Chantier::Projet(id) => {
            let nom = projets.iter().find(|p| p.0 == id).map_or(format!("Projet {id}"), |p| p.1.clone());
            format!("le chantier « {nom} »")
        }
    };
    let filtres_dits = format!("{}, {chantier}", c.filtres.periode.libelle().to_lowercase());
    Vue {
        depenses: calcul::indicateurs_des_depenses(&b),
        revenus: calcul::indicateurs_des_revenus(&b, c.filtres.periode),
        filtres: c.filtres,
        mois,
        chantiers,
        sections,
        fournisseurs,
        taxes,
        declaration,
        filtres_dits,
        bord: b,
    }
}

/// Le gris d'une catégorie de contexte (les autres), celui des graphiques.
fn graphiques_gris() -> Color32 {
    erp::GRIS.pastel
}

/// Le tableau de l'estimation : par trimestre, du plus récent au plus ancien (ceux qui portent une facture), les taxes
/// perçues, payées et nettes de chacune ; leur total en pied. Une seule taxe nommée : ses trois colonnes, la somme des
/// deux cases comme les tuiles — jamais trois colonnes sans nom (« perçue », « payée », « nette » : la QA5, passe 2).
fn tableau_de_la_declaration(b: &Bord) -> String {
    let (tps, tvq) = (&b.taxes.0, &b.taxes.1);
    let une_taxe = tvq.is_empty();
    let mut colonnes =
        vec!["Trimestre".to_string(), format!("{tps} perçue"), format!("{tps} payée"), format!("{tps} nette")];
    if !une_taxe {
        colonnes.extend([format!("{tvq} perçue"), format!("{tvq} payée"), format!("{tvq} nette")]);
    }
    let cellules = |nom: String, t: &[compta::Cents; 4]| -> Value {
        if une_taxe {
            let (percue, payee) = (t[0] + t[2], t[1] + t[3]);
            json!([nom, argent(percue), argent(payee), argent(percue - payee)])
        } else {
            json!([
                nom,
                argent(t[0]),
                argent(t[1]),
                argent(t[0] - t[1]),
                argent(t[2]),
                argent(t[3]),
                argent(t[2] - t[3])
            ])
        }
    };
    let mut total = [0; 4];
    let mut lignes = Vec::new();
    for t in b.trimestres.iter().rev().filter(|t| t.n_depenses + t.n_revenus > 0) {
        let x = [t.tps_percue, t.tps_payee, t.tvq_percue, t.tvq_payee];
        for (s, v) in total.iter_mut().zip(x) {
            *s += v;
        }
        let etat = match t.etat {
            EtatDuTrimestre::Clos => "",
            EtatDuTrimestre::EnCours => " (en cours)",
            EtatDuTrimestre::AVenir => " (à venir)",
        };
        lignes.push(cellules(format!("{}{etat}", t.court()), &x));
    }
    json!({ "colonnes": colonnes, "lignes": lignes, "pied": cellules("Total".into(), &total) }).to_string()
}

// ---- La page -------------------------------------------------------------------------------------------------------

/// Les cartes du tableau de bord.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum CarteDuBord {
    Mois,
    Saisie,
    Chantiers,
    Sections,
    Fournisseurs,
    Taxes,
    Declaration,
}

impl Vue {
    fn a(&self, k: CarteDuBord) -> bool {
        match k {
            CarteDuBord::Mois => self.mois.is_some(),
            CarteDuBord::Saisie => self.bord.depenses.n > 0,
            CarteDuBord::Chantiers => self.chantiers.is_some(),
            CarteDuBord::Sections => self.sections.is_some(),
            CarteDuBord::Fournisseurs => self.fournisseurs.is_some(),
            CarteDuBord::Taxes => self.taxes.is_some(),
            CarteDuBord::Declaration => self.declaration.is_some(),
        }
    }
}

/// Les cartes que montre la vue, dans leur ordre.
fn cartes_presentes(vue: &Vue) -> Vec<CarteDuBord> {
    use CarteDuBord::*;
    [Mois, Saisie, Chantiers, Sections, Fournisseurs, Taxes, Declaration].into_iter().filter(|k| vue.a(*k)).collect()
}

/// Les cartes en rangées : large, deux par rangée (le mois plus large que sa voisine, comme EG), l'estimation des
/// taxes seule ; étroite, une par rangée.
fn rangees(presentes: Vec<CarteDuBord>, etroite: bool) -> Vec<Vec<CarteDuBord>> {
    use CarteDuBord::*;
    let presentes = presentes.into_iter();
    if etroite {
        return presentes.map(|k| vec![k]).collect();
    }
    let mut toutes = Vec::new();
    let mut paire = Vec::new();
    for k in presentes {
        if k == Declaration {
            if !paire.is_empty() {
                toutes.push(std::mem::take(&mut paire));
            }
            toutes.push(vec![k]);
            continue;
        }
        paire.push(k);
        if paire.len() == 2 {
            toutes.push(std::mem::take(&mut paire));
        }
    }
    if !paire.is_empty() {
        toutes.push(paire);
    }
    toutes
}

/// La page du tableau de bord.
pub(super) fn page(f: &mut Fenetre, ui: &mut egui::Ui, c: &Contexte<'_>) {
    let vue = vue_en_memoire(ui.ctx(), c);
    let b = &vue.bord;
    if b.depenses.n == 0 && b.revenus.n == 0 {
        vide(ui, c);
        return;
    }
    // Les indicateurs des dépenses, puis ceux des revenus (sans facture client au registre, le mot d'EG).
    if b.depenses.n > 0 {
        if let Some(v) = tuiles(ui, "depenses", &vue.depenses, c.etroite) {
            aller_vers(ui.ctx(), v);
        }
    } else {
        let ici = if c.filtres.chantier == Chantier::Tous { "ici" } else { "pour ce chantier" };
        let periode = if c.filtres.periode == Periode::Tout { "" } else { " dans cette période" };
        note(ui, &format!("Aucune facture fournisseur {ici}{periode} : les dépenses et les taxes payées valent 0."));
    }
    ui.add_space(theme::ECART);
    if vue.revenus.is_empty() {
        note(ui, calcul::SANS_CLIENTS);
    } else if let Some(v) = tuiles(ui, "revenus", &vue.revenus, c.etroite) {
        aller_vers(ui.ctx(), v);
    }
    ui.add_space(theme::ECART);
    for cartes in rangees(cartes_presentes(&vue), c.etroite) {
        let parts: Vec<f32> =
            cartes.iter().map(|k| if *k == CarteDuBord::Mois && cartes.len() > 1 { 2.0 } else { 1.0 }).collect();
        let id = egui::Id::unique(("compta-bord-rangee", cartes.clone()));
        rangee(ui, id, &parts, |ui, j, hauteur| carte(f, ui, &vue, c, cartes[j], hauteur));
        ui.add_space(theme::ECART);
    }
}

/// Rien à montrer : aucune facture dans Constructo AI, aucune qui compte, ou aucune pour ces filtres (un bouton les
/// remet à tout).
fn vide(ui: &mut egui::Ui, c: &Contexte<'_>) {
    let tout = c.filtres == Filtres::default();
    let (titre, texte) = if c.registre.factures.is_empty() {
        (
            "Aucune facture",
            "Constructo AI n'a encore aucune facture : le tableau de bord se remplira avec les premières.",
        )
    } else if tout {
        (
            "Aucune facture à compter",
            "Les factures de Constructo AI sont annulées, pro forma, ou des brouillons aux clients.",
        )
    } else {
        ("Aucune facture ici", "Aucune facture pour ce chantier et cette période.")
    };
    let bouton = (!tout).then_some("Toute la période, tous les chantiers");
    if etat_vide(ui, pictos::GRAPHIQUE, erp::BLEU, titre, texte, bouton) {
        let ctx = ui.ctx().clone();
        super::choisir(&ctx, Choix { filtres: Filtres::default(), ..super::choix(&ctx) });
    }
}

/// Un clic qui mène ailleurs : une liste filtrée, ou une vue des sommaires.
fn aller_vers(ctx: &egui::Context, v: Vers) {
    let vers = |vue, etat| super::aller(ctx, Aller { vue, etat, ..Aller::default() });
    match v {
        Vers::Fournisseurs(etat) => vers(VueCompta::Fournisseurs, etat),
        Vers::Clients(etat) => vers(VueCompta::Clients, etat),
        Vers::ParMois => super::choisir(ctx, Choix { vue: VueCompta::ParMois, ..super::choix(ctx) }),
        Vers::ParChantier => super::choisir(ctx, Choix { vue: VueCompta::ParChantier, ..super::choix(ctx) }),
    }
}

/// Une note grise, qui passe à la ligne.
fn note(ui: &mut egui::Ui, texte: &str) {
    ui.add(egui::Label::new(RichText::new(lisible(texte)).size(12.5).color(erp::TEXTE_2)).wrap().selectable(false));
}

/// Un texte qui passe à la ligne sans couper un montant ni séparer un nombre du mot qui le suit (« 2 012 096,44 $ »,
/// « 104 factures », « 1 sans total ») : les insécables de la fenêtre (`natif::insecables` : l'espace fine, qu'egui
/// coupe, devient l'insécable, qu'il ne coupe pas), et une insécable après tout chiffre.
fn lisible(texte: &str) -> String {
    let s = super::super::insecables(texte);
    let mut lisible = String::with_capacity(s.len());
    let mut avant = ' ';
    for c in s.chars() {
        lisible.push(if c == ' ' && avant.is_ascii_digit() { '\u{a0}' } else { c });
        avant = c;
    }
    lisible
}

// ---- Les tuiles ----------------------------------------------------------------------------------------------------

/// L'icône et la teinte d'un indicateur.
fn allure(s: Sorte) -> (char, erp::Teinte) {
    match s {
        Sorte::Depenses => (pictos::FACTURES_RECUES, erp::BLEU),
        Sorte::AvantTaxes => (pictos::CALCUL, erp::INDIGO),
        Sorte::TaxesPayees => (pictos::BALANCE, erp::ORANGE),
        Sorte::Moyenne => (pictos::AGENDA, erp::SARCELLE),
        Sorte::AVerifier => (pictos::ALERTES, erp::AMBRE),
        Sorte::Revenus => (pictos::ENCAISSE, erp::VERT),
        Sorte::Encaisse => (pictos::PAIEMENTS, erp::VERT),
        Sorte::ARecevoir => (pictos::RETARD, erp::JAUNE),
        Sorte::Marge => (pictos::GRAPHIQUE, erp::VIOLET),
        Sorte::TaxesPercues => (pictos::BALANCE, erp::ORANGE),
        Sorte::TaxesNettes => (pictos::CALCUL, erp::GRIS),
    }
}

/// Les tuiles des indicateurs, en rangées de même hauteur (`theme::grille`, comme les tuiles du tableau de bord) :
/// toutes sur une rangée si elles y tiennent, par deux dans une fenêtre étroite. Rend où mène celle qu'on a touchée.
fn tuiles(ui: &mut egui::Ui, cle: &str, liste: &[Indicateur], etroite: bool) -> Option<Vers> {
    let par_rangee = if etroite { 2 } else { theme::tuiles_par_rangee(liste.len(), ui.available_width()) };
    let id = egui::Id::unique(("compta-tuiles", cle, par_rangee, liste.len()));
    let mut touchee = None;
    theme::grille(ui, id, par_rangee, liste.len(), |ui, k, hauteur| {
        let (h, clic) = tuile(ui, &liste[k], hauteur, egui::Id::unique(("compta-tuile", cle, k)));
        if clic {
            touchee = liste[k].vers;
        }
        h
    });
    touchee
}

/// Une tuile, comme celles du tableau de bord (`theme::tuiles`) — sa pastille, son titre, sa valeur en grand, son
/// détail —, qu'on touche quand elle mène quelque part (la main, sa bulle). Rend la hauteur qu'il lui faut, et le clic.
fn tuile(ui: &mut egui::Ui, x: &Indicateur, hauteur: f32, id: egui::Id) -> (f32, bool) {
    let (coin, largeur) = (ui.cursor().min, ui.available_width());
    let (icone, teinte) = allure(x.sorte);
    let teinte = if x.alerte { erp::ROUGE } else { teinte };
    let naturelle = theme::carte(ui, None, hauteur, |ui| {
        let largeur = ui.available_width();
        let serree = largeur < 170.0;
        let (cote, ecart, grande) = if serree { (32.0, 10.0, 21.0) } else { (40.0, 14.0, 24.0) };
        let l_texte = (largeur - cote - ecart).max(40.0);
        let p = ui.painter().clone();
        let titre =
            p.layout(lisible(&x.titre), FontId::new(12.5, FontFamily::Name(SEMI.into())), erp::TEXTE_2, l_texte);
        let gras = |t: f32| FontId::new(t, FontFamily::Name(GRAS.into()));
        let mesure = p.layout_no_wrap(x.valeur.clone(), gras(grande), erp::TITRE).size().x;
        let taille = if mesure > l_texte { (grande * l_texte / mesure).max(14.0) } else { grande };
        let mut job = egui::text::LayoutJob::simple_singleline(x.valeur.clone(), gras(taille), erp::TITRE);
        job.wrap = egui::text::TextWrapping::truncate_at_width(l_texte);
        let valeur = p.layout_job(job);
        let detail = p.layout(lisible(&x.detail), FontId::proportional(12.5), erp::TEXTE_2, l_texte);
        let (h_titre, h_valeur) = (titre.size().y, valeur.size().y);
        let h = (h_titre + h_valeur + detail.size().y + 2.0).max(cote);
        let (r, _) = ui.allocate_exact_size(vec2(largeur, h), Sense::hover());
        theme::pastille(&p, Rect::from_min_size(r.min, vec2(cote, cote)), icone, teinte);
        let x0 = r.left() + cote + ecart;
        p.galley(pos2(x0, r.top() - 2.0), titre, erp::TEXTE_2);
        p.galley(pos2(x0, r.top() - 2.0 + h_titre), valeur, erp::TITRE);
        p.galley(pos2(x0, r.top() + h_titre + h_valeur), detail, erp::TEXTE_2);
    });
    if x.vers.is_none() {
        return (naturelle, false);
    }
    let rect = Rect::from_min_size(coin, vec2(largeur, naturelle.max(hauteur)));
    let rep = ui.interact(rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    let rep = if x.bulle.is_empty() { rep } else { rep.on_hover_text(x.bulle) };
    (naturelle, rep.clicked())
}

// ---- Les cartes ----------------------------------------------------------------------------------------------------

/// Une rangée de cartes côte à côte, de largeurs `parts` (des fractions de la largeur), à `theme::ECART` l'une de
/// l'autre et de même hauteur — celle de la plus haute à l'image d'avant, comme `theme::grille`. `carte(ui, k,
/// hauteur)` dessine la k-ième et rend la hauteur qu'il lui faut.
fn rangee(ui: &mut egui::Ui, id: egui::Id, parts: &[f32], mut carte: impl FnMut(&mut egui::Ui, usize, f32) -> f32) {
    let (origine, largeur) = (ui.cursor().min, ui.available_width());
    let utile = largeur - theme::ECART * parts.len().saturating_sub(1) as f32;
    let somme: f32 = parts.iter().sum::<f32>().max(1e-3);
    let avant: f32 = ui.ctx().data(|d| d.get_temp(id)).unwrap_or(0.0);
    let (mut x, mut bas, mut haute) = (origine.x, origine.y, 0.0_f32);
    for (k, part) in parts.iter().enumerate() {
        let l = (utile * part / somme).max(40.0);
        let place = Rect::from_min_size(pos2(x, origine.y), vec2(l, 100_000.0));
        let mut enfant = ui.new_child(UiBuilder::new().max_rect(place).layout(Layout::top_down(Align::Min)));
        haute = haute.max(carte(&mut enfant, k, avant));
        bas = bas.max(enfant.min_rect().bottom());
        x += l + theme::ECART;
    }
    ui.advance_cursor_after_rect(Rect::from_min_max(origine, pos2(origine.x + largeur, bas)));
    if (haute - avant).abs() > 0.5 {
        ui.ctx().data_mut(|d| d.insert_temp(id, haute));
        ui.ctx().request_repaint();
    }
}

/// La tête d'une carte : sa clé, son titre, son icône et sa teinte, ce que dit son sous-titre.
struct Tete {
    cle: &'static str,
    titre: String,
    icone: char,
    teinte: erp::Teinte,
    sous_titre: String,
}

/// Une carte de graphique : sa tête et son menu « ⋯ » (voir les données — la vue en tableau, où chaque valeur se lit
/// au cent sans survol — ou le graphique ; en parler à Eve), son sous-titre, puis son corps (`en_tableau` : la vue en
/// tableau est demandée). Rend la hauteur qu'il lui faut.
fn carte_de_graphique(
    f: &mut Fenetre,
    ui: &mut egui::Ui,
    t: &Tete,
    filtres_dits: &str,
    hauteur: f32,
    corps: impl FnOnce(&mut Fenetre, &mut egui::Ui, bool),
) -> f32 {
    let id = egui::Id::unique(("compta-bord-en-tableau", t.cle));
    let en_tableau = ui.ctx().data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let (icone, mot) = if en_tableau {
        (pictos::GRAPHIQUE, "Voir le graphique")
    } else {
        (crate::cartes::pictos::TABLEAU, "Voir les données")
    };
    let lignes = [
        LigneDeMenu { icone, mot, a_confirmer: false },
        LigneDeMenu { icone: pictos::PARLER, mot: "En parler à Eve", a_confirmer: false },
    ];
    let mut choisie = None;
    let h = theme::carte(ui, Some(t.teinte.pastel), hauteur, |ui| {
        let (ligne, _) = theme::tete_avec_menu(ui, &t.titre, t.icone, t.teinte, t.cle, &lignes);
        choisie = ligne;
        if !t.sous_titre.is_empty() {
            note(ui, &t.sous_titre);
        }
        ui.add_space(6.0);
        corps(f, ui, en_tableau);
    });
    match choisie {
        Some(0) => {
            ui.ctx().data_mut(|d| d.insert_temp(id, !en_tableau));
        }
        Some(1) => {
            let demande =
                format!("Parle-moi de « {} » dans la comptabilité de l'entreprise ({filtres_dits}).", t.titre);
            f.geste("texte", Value::String(demande));
        }
        _ => {}
    }
    h
}

/// La carte `k`.
fn carte(f: &mut Fenetre, ui: &mut egui::Ui, vue: &Vue, c: &Contexte<'_>, k: CarteDuBord, hauteur: f32) -> f32 {
    let b = &vue.bord;
    let ctx = ui.ctx().clone();
    let dits = vue.filtres_dits.as_str();
    match k {
        CarteDuBord::Mois => {
            let Some(mois) = &vue.mois else { return 0.0 };
            let titre = if b.revenus.n > 0 { "Dépenses et revenus par mois" } else { "Dépenses par mois" };
            let mut sous_titre = calcul::sous_titre_des_mois(b, vue.filtres.periode);
            sous_titre += " Touche une colonne pour voir ses factures.";
            let t = Tete {
                cle: "compta-mois",
                titre: titre.into(),
                icone: pictos::GRAPHIQUE,
                teinte: erp::BLEU,
                sous_titre,
            };
            carte_de_graphique(f, ui, &t, dits, hauteur, |f, ui, en_tableau| {
                if en_tableau {
                    f.cartes.tableau_seul(ui, &mois.graphique.en_tableau());
                } else if let Some((cle, serie)) = mois.toucher(ui, "compta-mois") {
                    // La colonne des revenus mène aux factures des clients ; celle des dépenses, aux fournisseurs.
                    let vue = if serie == Some(1) { VueCompta::Clients } else { VueCompta::Fournisseurs };
                    super::aller(&ctx, Aller { vue, mois: Some(cle), ..Aller::default() });
                }
                if let Some(l) = calcul::dernier_mois(b) {
                    ui.add_space(4.0);
                    let l =
                        RichText::new(lisible(&l)).size(13.0).family(FontFamily::Name(SEMI.into())).color(erp::TEXTE);
                    ui.add(egui::Label::new(l).wrap().selectable(false));
                }
                for n in calcul::notes_des_mois(b) {
                    note(ui, &n);
                }
            })
        }
        CarteDuBord::Saisie => carte_de_la_saisie(ui, b, hauteur),
        CarteDuBord::Chantiers => {
            let Some(chantiers) = &vue.chantiers else { return 0.0 };
            let n = b.chantiers.len() + b.autres_chantiers.as_ref().map_or(0, |(n, _)| *n as usize);
            let sous_titre = format!(
                "{}, du plus coûteux au moins coûteux. Touches-en un pour le choisir en tête.",
                calcul::pluriel(n as u32, "chantier", "chantiers")
            );
            let t = Tete {
                cle: "compta-chantiers",
                titre: "Par chantier".into(),
                icone: pictos::PROJETS,
                teinte: erp::SARCELLE,
                sous_titre,
            };
            carte_de_graphique(f, ui, &t, dits, hauteur, |f, ui, en_tableau| {
                if en_tableau {
                    f.cartes.tableau_seul(ui, &chantiers.graphique.en_tableau());
                } else if let Some((chantier, _)) = chantiers.toucher(ui, "compta-chantiers") {
                    let actuel = super::choix(&ctx);
                    let filtres = Filtres { chantier, ..actuel.filtres };
                    super::choisir(&ctx, Choix { filtres, ..actuel });
                }
            })
        }
        CarteDuBord::Sections => {
            let Some((anneau, liste)) = &vue.sections else { return 0.0 };
            let sous_titre = "Les dépenses de chaque section, de la plus grosse à la plus petite. Touches-en une pour \
                              voir ses factures."
                .to_string();
            let t = Tete {
                cle: "compta-sections",
                titre: "Par corps de métier".into(),
                icone: pictos::BALANCE,
                teinte: erp::VIOLET,
                sous_titre,
            };
            carte_de_graphique(f, ui, &t, dits, hauteur, |f, ui, en_tableau| {
                if en_tableau {
                    f.cartes.tableau_seul(ui, &liste.graphique.en_tableau());
                    return;
                }
                // Large, l'anneau à gauche de la liste ; étroite, au-dessus.
                let largeur = ui.available_width();
                let cote_a_cote = largeur >= 520.0;
                if let Some(a) = anneau {
                    if cote_a_cote {
                        let debut = ui.cursor().min;
                        let place = Rect::from_min_size(debut, vec2(160.0, 160.0));
                        let mut gauche =
                            ui.new_child(UiBuilder::new().max_rect(place).layout(Layout::top_down(Align::Min)));
                        toucher_une_section(&ctx, &mut gauche, a, "compta-sections-anneau");
                        let reste = Rect::from_min_max(
                            pos2(debut.x + 180.0, debut.y),
                            pos2(debut.x + largeur, debut.y + 100_000.0),
                        );
                        let mut droite =
                            ui.new_child(UiBuilder::new().max_rect(reste).layout(Layout::top_down(Align::Min)));
                        toucher_une_section(&ctx, &mut droite, liste, "compta-sections");
                        let bas = gauche.min_rect().bottom().max(droite.min_rect().bottom());
                        ui.advance_cursor_after_rect(Rect::from_min_max(debut, pos2(debut.x + largeur, bas)));
                        return;
                    }
                    toucher_une_section(&ctx, ui, a, "compta-sections-anneau");
                    ui.add_space(8.0);
                }
                toucher_une_section(&ctx, ui, liste, "compta-sections");
            })
        }
        CarteDuBord::Fournisseurs => {
            let Some(fournisseurs) = &vue.fournisseurs else { return 0.0 };
            let n = b.fournisseurs.len() + b.autres_fournisseurs.as_ref().map_or(0, |(n, _)| *n as usize);
            let sous_titre = if n > b.fournisseurs.len() {
                format!(
                    "Les {} plus importants, sur {}. Touches-en un pour voir ses factures.",
                    b.fournisseurs.len(),
                    crate::compta::par_milliers(n as u64)
                )
            } else {
                format!(
                    "{}. Touches-en un pour voir ses factures.",
                    calcul::pluriel(n as u32, "fournisseur", "fournisseurs")
                )
            };
            let t = Tete {
                cle: "compta-fournisseurs",
                titre: "Principaux fournisseurs".into(),
                icone: pictos::COMMANDES,
                teinte: erp::INDIGO,
                sous_titre,
            };
            carte_de_graphique(f, ui, &t, dits, hauteur, |f, ui, en_tableau| {
                if en_tableau {
                    f.cartes.tableau_seul(ui, &fournisseurs.graphique.en_tableau());
                } else if let Some((nom, _)) = fournisseurs.toucher(ui, "compta-fournisseurs") {
                    super::aller(&ctx, Aller { vue: VueCompta::Fournisseurs, tiers: Some(nom), ..Aller::default() });
                }
            })
        }
        CarteDuBord::Taxes => {
            let Some(g) = &vue.taxes else { return 0.0 };
            let sous_titre = if b.taxes.1.is_empty() {
                format!("La {} inscrite sur les factures reçues.", b.taxes.0)
            } else {
                format!("La {} et la {} inscrites sur les factures reçues.", b.taxes.0, b.taxes.1)
            };
            let t = Tete {
                cle: "compta-taxes",
                titre: "Taxes payées par trimestre".into(),
                icone: pictos::BALANCE,
                teinte: erp::ORANGE,
                sous_titre,
            };
            carte_de_graphique(f, ui, &t, dits, hauteur, |f, ui, en_tableau| {
                if en_tableau {
                    f.cartes.tableau_seul(ui, &g.en_tableau());
                } else {
                    graphiques::graphique(ui, g, "compta-taxes");
                }
                // Les mêmes trimestres que l'estimation : ce qu'ils ne montrent pas se dit ici aussi (un trimestre
                // écarté, les plus anciens, les factures datées après le dernier).
                for n in calcul::notes_des_trimestres(b) {
                    note(ui, &n);
                }
            })
        }
        CarteDuBord::Declaration => {
            let Some((g, tableau)) = &vue.declaration else { return 0.0 };
            // « TPS et TVQ à remettre, estimées », « TVH à remettre, estimée » : une seule taxe se dit seule (la QA5,
            // passe 2 : « TPS et  à remettre »).
            let estimees = if b.taxes.1.is_empty() && !b.taxes.0.contains('/') { "estimée" } else { "estimées" };
            let titre = if c.filtres.chantier == Chantier::Tous {
                format!("{} à remettre, {estimees}", b.les_taxes("et"))
            } else {
                format!("{} à remettre, {estimees} : ce chantier seulement", b.les_taxes("et"))
            };
            let sous_titre = "Par trimestre : les taxes perçues sur les factures des clients, moins celles payées sur \
                              les factures des fournisseurs. Au-dessus de zéro, à remettre ; au-dessous, elles \
                              reviennent à l'entreprise."
                .to_string();
            let t = Tete { cle: "compta-declaration", titre, icone: pictos::CALCUL, teinte: erp::VIOLET, sous_titre };
            carte_de_graphique(f, ui, &t, dits, hauteur, |f, ui, en_tableau| {
                if en_tableau {
                    f.cartes.tableau_seul(ui, &g.en_tableau());
                } else {
                    // Large, le graphique à gauche du tableau ; étroite, au-dessus.
                    let largeur = ui.available_width();
                    if largeur >= 1000.0 {
                        let debut = ui.cursor().min;
                        let l = (largeur - theme::ECART) * 0.45;
                        let place = Rect::from_min_size(debut, vec2(l, 100_000.0));
                        let mut gauche =
                            ui.new_child(UiBuilder::new().max_rect(place).layout(Layout::top_down(Align::Min)));
                        graphiques::graphique(&mut gauche, g, "compta-declaration");
                        let reste = Rect::from_min_max(
                            pos2(debut.x + l + theme::ECART, debut.y),
                            pos2(debut.x + largeur, debut.y + 100_000.0),
                        );
                        let mut droite =
                            ui.new_child(UiBuilder::new().max_rect(reste).layout(Layout::top_down(Align::Min)));
                        f.cartes.tableau_seul(&mut droite, tableau);
                        let bas = gauche.min_rect().bottom().max(droite.min_rect().bottom());
                        ui.advance_cursor_after_rect(Rect::from_min_max(debut, pos2(debut.x + largeur, bas)));
                    } else {
                        graphiques::graphique(ui, g, "compta-declaration");
                        ui.add_space(8.0);
                        f.cartes.tableau_seul(ui, tableau);
                    }
                }
                ui.add_space(4.0);
                for n in calcul::notes_de_la_declaration(b, vue.filtres) {
                    note(ui, &n);
                }
            })
        }
    }
}

/// L'anneau ou la liste des sections : un clic sur une part ou une rangée ouvre les factures de sa section.
fn toucher_une_section(ctx: &egui::Context, ui: &mut egui::Ui, sections: &Touchable<String>, cle: &str) {
    if let Some((section, _)) = sections.toucher(ui, cle) {
        super::aller(ctx, Aller { vue: VueCompta::Fournisseurs, section: Some(section), ..Aller::default() });
    }
}

/// L'état de la saisie : la fiabilité des factures reçues (sûre, à vérifier, non jugée) en une barre et ses rangées,
/// les brouillons et les montants qui manquent ; ce qui reste à vérifier et les brouillons mènent à leurs factures.
fn carte_de_la_saisie(ui: &mut egui::Ui, b: &Bord, hauteur: f32) -> f32 {
    let ctx = ui.ctx().clone();
    theme::carte(ui, Some(erp::AMBRE.pastel), hauteur, |ui| {
        theme::tete_de_carte(ui, "État de la saisie", pictos::LISTE, erp::AMBRE, false, None);
        note(ui, "La fiabilité de la saisie des factures reçues, et ce qui reste à compléter.");
        ui.add_space(8.0);
        let n = b.depenses.n;
        let parts = [
            (b.surs, SURE, "Sûre", None),
            (b.a_verifier, A_VERIFIER, "À vérifier", Some("a_verifier")),
            (b.non_juges, NON_JUGEE, "Non jugée", None),
        ];
        let (barre, _) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0), Sense::hover());
        let valeurs: Vec<(f64, Color32)> = parts.iter().map(|(k, c, _, _)| (f64::from(*k), *c)).collect();
        theme::barre_empilee(ui.painter(), barre, &valeurs);
        ui.add_space(6.0);
        for (k, couleur, mot, etat) in parts {
            let part = calcul::part(i64::from(k), i64::from(n)).map(|p| format!(" · {p}")).unwrap_or_default();
            let chiffre = format!("{}{part}", calcul::pluriel(k, "facture", "factures"));
            let cliquable = etat.is_some() && k > 0;
            if rangee_de_saisie(ui, Some(couleur), mot, &chiffre, ("saisie", mot), cliquable) {
                super::aller(&ctx, Aller { vue: VueCompta::Fournisseurs, etat, ..Aller::default() });
            }
        }
        ui.add_space(6.0);
        let brouillons = calcul::pluriel(b.brouillons, "facture", "factures");
        if rangee_de_saisie(ui, None, "En brouillon", &brouillons, ("saisie", "brouillons"), b.brouillons > 0) {
            super::aller(&ctx, Aller { vue: VueCompta::Fournisseurs, etat: Some("brouillons"), ..Aller::default() });
        }
        // Les montants qui manquent : ils comptent pour 0 dans les sommes.
        let taxes = format!("Sans {}", b.les_taxes("ou"));
        let manques = [
            ("Sans total", b.depenses.sans_total),
            ("Sans sous-total", b.depenses_sans_sous_total),
            (taxes.as_str(), b.depenses_sans_taxe),
        ];
        if manques.iter().all(|(_, k)| *k == 0) {
            rangee_de_saisie(ui, None, "Aucun montant manquant", "", ("saisie", "manques"), false);
            return;
        }
        for (mot, k) in manques.into_iter().filter(|(_, k)| *k > 0) {
            let chiffre = calcul::pluriel(k, "facture", "factures");
            rangee_de_saisie(ui, None, mot, &chiffre, ("saisie", mot), false);
        }
        note(ui, "Un montant manquant compte pour 0 dans les sommes.");
    })
}

/// Une rangée de l'état de la saisie : sa pastille (au besoin), son mot, son chiffre à droite ; cliquable, la main, un
/// lavis au survol et un chevron. Rend vrai si on l'a touchée.
fn rangee_de_saisie(
    ui: &mut egui::Ui,
    couleur: Option<Color32>,
    mot: &str,
    chiffre: &str,
    cle: (&str, &str),
    cliquable: bool,
) -> bool {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    let rep = cliquable.then(|| {
        ui.interact(r, egui::Id::unique(("compta-rangee-de-saisie", cle.0, cle.1)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
    });
    let p = ui.painter();
    if rep.as_ref().is_some_and(egui::Response::hovered) {
        p.rect_filled(r, 6.0, erp::SURVOL);
    }
    let mut x = r.left() + 6.0;
    if let Some(c) = couleur {
        p.rect_filled(Rect::from_center_size(pos2(x + 5.0, r.center().y), vec2(10.0, 10.0)), 3.0, c);
        x += 18.0;
    }
    p.text(pos2(x, r.center().y), egui::Align2::LEFT_CENTER, mot, FontId::proportional(13.0), erp::TEXTE);
    let droite = if cliquable { r.right() - 24.0 } else { r.right() - 6.0 };
    let police = FontId::new(13.0, FontFamily::Name(SEMI.into()));
    p.text(pos2(droite, r.center().y), egui::Align2::RIGHT_CENTER, chiffre, police, erp::TITRE);
    if cliquable {
        let chevron = FontId::new(11.0, FontFamily::Name(ICONES.into()));
        p.text(pos2(r.right() - 10.0, r.center().y), egui::Align2::CENTER_CENTER, '\u{E76C}', chevron, erp::TEXTE_2);
    }
    rep.is_some_and(|r| r.clicked())
}

#[cfg(test)]
mod essais {
    use super::*;
    use crate::compta::bord::Trimestre;

    /// Le tableau de l'estimation : le trimestre le plus récent d'abord, celui d'aujourd'hui dit « en cours », les
    /// nettes (perçue moins payée) et leur total au cent.
    #[test]
    fn le_tableau_de_l_estimation() {
        let trimestre = |t, tps_percue, tps_payee, tvq_percue, tvq_payee, n_revenus, etat| Trimestre {
            annee: 2026,
            t,
            tps_percue,
            tps_payee,
            tvq_percue,
            tvq_payee,
            n_depenses: 1,
            n_revenus,
            etat,
        };
        let b = Bord {
            taxes: ("TPS".into(), "TVQ".into()),
            trimestres: vec![
                trimestre(2, 0, 0, 0, 0, 0, EtatDuTrimestre::Clos),
                trimestre(3, 35_000, 15_000, 69_825, 29_925, 2, EtatDuTrimestre::Clos),
                trimestre(4, 2_500, 4_000, 4_987, 2_993, 1, EtatDuTrimestre::EnCours),
            ],
            ..Bord::default()
        };
        // Le T2 porte une facture (n_depenses : 1) : il reste, à zéro.
        let t: Value = serde_json::from_str(&tableau_de_la_declaration(&b)).unwrap();
        let a = |c: compta::Cents| argent(c);
        assert_eq!(t["colonnes"][3], "TPS nette");
        assert_eq!(t["lignes"].as_array().map(Vec::len), Some(3));
        // T4 : TPS 25,00 − 40,00 = −15,00 $ (elle revient) ; TVQ 49,87 − 29,93 = 19,94 $.
        assert_eq!(
            t["lignes"][0],
            json!(["T4 2026 (en cours)", a(2_500), a(4_000), a(-1_500), a(4_987), a(2_993), a(1_994)])
        );
        assert_eq!(t["lignes"][1][0], "T3 2026");
        assert_eq!(t["lignes"][2][0], "T2 2026");
        // Le total : TPS 375,00 − 190,00 = 185,00 $ ; TVQ 748,12 − 329,18 = 418,94 $.
        assert_eq!(t["pied"], json!(["Total", a(37_500), a(19_000), a(18_500), a(74_812), a(32_918), a(41_894)]));
    }

    /// La QA5 (passe 2) : une seule taxe nommée (la TVH, une entreprise à la TPS seule) — ses trois colonnes sous son
    /// nom, la somme des deux cases ; jamais « perçue », « payée », « nette » sans nom.
    #[test]
    fn le_tableau_d_une_seule_taxe() {
        let t4 = Trimestre {
            annee: 2026,
            t: 4,
            tps_percue: 2_600,
            tps_payee: 1_300,
            tvq_percue: 100,
            tvq_payee: 0,
            n_depenses: 1,
            n_revenus: 1,
            etat: EtatDuTrimestre::EnCours,
        };
        let b = Bord { taxes: ("TVH".into(), String::new()), trimestres: vec![t4], ..Bord::default() };
        let t: Value = serde_json::from_str(&tableau_de_la_declaration(&b)).unwrap();
        let a = |c: compta::Cents| argent(c);
        assert_eq!(t["colonnes"], json!(["Trimestre", "TVH perçue", "TVH payée", "TVH nette"]));
        assert_eq!(t["lignes"][0], json!(["T4 2026 (en cours)", a(2_700), a(1_300), a(1_400)]));
        assert_eq!(t["pied"], json!(["Total", a(2_700), a(1_300), a(1_400)]));
    }

    /// Un texte qui passe à la ligne ne coupe ni un montant (l'espace fine devient insécable) ni un nombre de son mot.
    #[test]
    fn un_texte_lisible() {
        assert_eq!(
            lisible("104 factures · avant taxes 2\u{202f}012\u{202f}096,44\u{a0}$ · 1 sans total"),
            "104\u{a0}factures · avant taxes 2\u{a0}012\u{a0}096,44\u{a0}$ · 1\u{a0}sans total"
        );
        assert_eq!(lisible("TPS 5,00 $ : à remettre"), "TPS 5,00\u{a0}$\u{a0}: à remettre");
    }

    /// Large, deux cartes par rangée, l'estimation des taxes seule (même après un nombre impair de cartes) ; étroite,
    /// une par rangée.
    #[test]
    fn les_cartes_en_rangees() {
        use CarteDuBord::*;
        let toutes = vec![Mois, Saisie, Chantiers, Sections, Fournisseurs, Taxes, Declaration];
        assert_eq!(
            rangees(toutes.clone(), false),
            [vec![Mois, Saisie], vec![Chantiers, Sections], vec![Fournisseurs, Taxes], vec![Declaration]]
        );
        let sans_chantiers = vec![Mois, Saisie, Sections, Fournisseurs, Taxes, Declaration];
        assert_eq!(
            rangees(sans_chantiers, false),
            [vec![Mois, Saisie], vec![Sections, Fournisseurs], vec![Taxes], vec![Declaration]]
        );
        assert_eq!(rangees(toutes, true).len(), 7);
    }
}
