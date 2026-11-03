//! Le calcul du tableau de bord de l'onglet « Comptabilité », celui d'Entrepreneur Général (« plein de beaux
//! graphismes, tableaux, pour bien voir de façon intuitive », la demande) : les indicateurs et les séries de ses
//! graphiques, d'après les factures que retiennent les filtres d'en tête ([`Filtres::retient`]) et les règles du
//! contrat ([`Facture::depense`], [`Facture::revenu`], [`Facture::signe`], [`Facture::reste`], [`Facture::en_retard`],
//! [`Somme`]), en centimes. Pur : ni egui, ni fichier, ni horloge — le jour arrive en argument. Ce qu'il dit aussi se
//! calcule ici (les indicateurs, les notes des graphiques), pour s'essayer mot pour mot. Son dessin :
//! `natif\compta\bord.rs`.
//!
//! L'ERP n'a ni « extra » ni « vérifiée par Mario », et sa fiabilité n'a que deux valeurs (`sur`, `a_verifier`) : le
//! tableau de bord montre ce que l'ERP a aujourd'hui, sans rien inventer.

use std::cmp::Ordering;
use std::collections::HashMap;

use super::{Cents, Chantier, Facture, Filtres, MOIS, NomsDesTaxes, Periode, Registre, Somme, argent};

/// Au plus, les mois que dessine le graphique par mois (comme EG) ; les plus anciens se disent.
pub const MOIS_MAX: usize = 48;
/// Les principaux fournisseurs ; les autres, en une ligne.
pub const FOURNISSEURS_MAX: usize = 10;
/// Les chantiers, du plus coûteux au moins coûteux ; les autres, en une ligne.
pub const CHANTIERS_MAX: usize = 12;
/// Au plus, les trimestres des taxes (comme EG) ; les plus anciens se disent.
pub const TRIMESTRES_MAX: usize = 16;
/// Les mois de chaque trimestre.
const MOIS_DES_TRIMESTRES: [&str; 4] = ["janv.–mars", "avr.–juin", "juil.–sept.", "oct.–déc."];
/// Le mot d'EG quand le registre n'a aucune facture client.
pub const SANS_CLIENTS: &str = "Aucune facture client : les revenus, la marge et les taxes perçues s'ajouteront ici.";

// ---- Ce qu'il calcule -----------------------------------------------------------------------------------------------

/// La fiabilité de la saisie d'une facture reçue, telle que l'ERP la garde (`sur`, `a_verifier`) ; sans valeur (ou
/// d'une valeur que l'ERP ne donne pas), elle n'est pas jugée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fiabilite {
    Sure,
    AVerifier,
    NonJugee,
}

/// La fiabilité d'une facture : la valeur de l'ERP, sans égard à la casse, aux accents ni au trait (« à vérifier »,
/// « A-verifier » valent `a_verifier`).
pub fn fiabilite(f: &Facture) -> Fiabilite {
    let Some(v) = f.fiabilite.as_deref() else { return Fiabilite::NonJugee };
    let v: String = v
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'à' | 'â' => 'a',
            'é' | 'è' | 'ê' => 'e',
            'û' | 'ù' => 'u',
            ' ' | '-' => '_',
            c => c,
        })
        .collect();
    match v.as_str() {
        "sur" | "sure" => Fiabilite::Sure,
        "a_verifier" => Fiabilite::AVerifier,
        _ => Fiabilite::NonJugee,
    }
}

/// Un mois du graphique : ses dépenses et ses revenus (signés : une note de crédit se retranche), et combien de
/// factures de chaque côté.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mois {
    pub annee: i32,
    pub mois: u8,
    pub depenses: Cents,
    pub n_depenses: u32,
    pub revenus: Cents,
    pub n_revenus: u32,
}

impl Mois {
    fn vide(rang: i32) -> Mois {
        Mois { annee: rang.div_euclid(12), mois: (rang.rem_euclid(12) + 1) as u8, ..Mois::default() }
    }

    /// « 2026-09 » : le mois d'une liste filtrée (`Aller::mois`).
    pub fn cle(&self) -> String {
        format!("{:04}-{:02}", self.annee, self.mois)
    }

    /// « septembre 2026 ».
    pub fn nom(&self) -> String {
        format!("{} {}", MOIS[usize::from(self.mois.clamp(1, 12)) - 1], self.annee)
    }
}

/// Un regroupement de dépenses (un corps de métier, un fournisseur) : son nom tel qu'écrit, son total signé, ses
/// factures.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Groupe {
    pub nom: String,
    pub total: Cents,
    pub n: u32,
}

/// Un chantier : ses dépenses et ses revenus. `chantier` : celui que le clic choisit en tête (`Chantier::Tous` pour la
/// ligne qui réunit les autres).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LigneDeChantier {
    pub chantier: Chantier,
    pub nom: String,
    pub depenses: Cents,
    pub n_depenses: u32,
    pub revenus: Cents,
    pub n_revenus: u32,
}

impl LigneDeChantier {
    fn de(chantier: Chantier, nom: String) -> LigneDeChantier {
        LigneDeChantier { chantier, nom, depenses: 0, n_depenses: 0, revenus: 0, n_revenus: 0 }
    }
}

/// Un trimestre est-il clos, en cours (celui d'aujourd'hui), ou à venir (une facture datée plus tard) ?
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EtatDuTrimestre {
    #[default]
    Clos,
    EnCours,
    AVenir,
}

/// Un trimestre : les taxes payées (inscrites sur les factures reçues) et perçues (sur les factures des clients).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Trimestre {
    pub annee: i32,
    /// 1 à 4.
    pub t: u8,
    pub tps_payee: Cents,
    pub tvq_payee: Cents,
    pub n_depenses: u32,
    pub tps_percue: Cents,
    pub tvq_percue: Cents,
    pub n_revenus: u32,
    pub etat: EtatDuTrimestre,
}

impl Trimestre {
    /// Le trimestre de rang `rang` (le rang de son premier mois divisé par trois), `courant` : le rang du mois
    /// d'aujourd'hui.
    fn vide(rang: i32, courant: i32) -> Trimestre {
        let etat = match rang.cmp(&courant.div_euclid(3)) {
            Ordering::Less => EtatDuTrimestre::Clos,
            Ordering::Equal => EtatDuTrimestre::EnCours,
            Ordering::Greater => EtatDuTrimestre::AVenir,
        };
        Trimestre { annee: rang.div_euclid(4), t: (rang.rem_euclid(4) + 1) as u8, etat, ..Trimestre::default() }
    }

    /// « T3 2026 ».
    pub fn court(&self) -> String {
        format!("T{} {}", self.t, self.annee)
    }

    /// « juil.–sept. ».
    pub fn mois(&self) -> &'static str {
        MOIS_DES_TRIMESTRES[usize::from(self.t.clamp(1, 4)) - 1]
    }

    /// Perçue moins payée : positive, à remettre ; négative, elle revient à l'entreprise.
    pub fn tps_nette(&self) -> Cents {
        self.tps_percue - self.tps_payee
    }

    pub fn tvq_nette(&self) -> Cents {
        self.tvq_percue - self.tvq_payee
    }
}

/// Le tableau de bord, calculé : les indicateurs et les séries de ses graphiques.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bord {
    // Les dépenses : les factures reçues des fournisseurs (`Facture::depense` : un brouillon compte).
    pub depenses: Somme,
    pub brouillons: u32,
    pub surs: u32,
    pub a_verifier: u32,
    pub non_juges: u32,
    /// La moyenne des dépenses par mois, au cent, sur les mois qui en ont (`mois_actifs`) ; une facture sans date n'a
    /// pas de mois.
    pub moyenne: Cents,
    pub mois_actifs: u32,
    // Les revenus : les factures émises aux clients (`Facture::revenu`).
    pub revenus: Somme,
    /// Le registre a-t-il au moins un revenu, toutes périodes et tous chantiers confondus ? Sinon, le mot d'EG
    /// ([`SANS_CLIENTS`]) remplace les indicateurs des revenus.
    pub clients_inscrits: bool,
    /// Ce que les clients ont payé (leur `paye`, une note de crédit remboursée se retranche), et les factures réglées.
    pub encaisse: Cents,
    pub reglees: u32,
    /// Ce qui reste à recevoir (`Facture::reste`), et ce qui en est en retard au jour donné.
    pub a_recevoir: Cents,
    pub n_a_recevoir: u32,
    pub en_retard: Cents,
    pub n_en_retard: u32,
    /// Les factures (dépenses et revenus) sans sous-total, ou sans TPS ou sans TVQ : elles comptent pour 0 là ; et,
    /// pour l'état de la saisie, celles des dépenses seules.
    pub sans_sous_total: u32,
    pub sans_taxe: u32,
    pub depenses_sans_sous_total: u32,
    pub depenses_sans_taxe: u32,
    /// Les mois dessinés, du plus ancien au plus récent ([`MOIS_MAX`] au plus) ; ceux d'avant, cachés : combien,
    /// combien d'entre eux ont des dépenses (elles comptent dans la moyenne), et leurs revenus.
    pub mois: Vec<Mois>,
    pub mois_caches: u32,
    pub mois_caches_avec_depenses: u32,
    pub revenus_caches: (u32, Cents),
    /// Les factures retenues sans date (seulement dans « Toute la période ») : elles n'ont pas de mois.
    pub sans_date: u32,
    /// Les factures retenues datées après la fenêtre des mois — dans « Toute la période », après le mois en cours (une
    /// faute de frappe : 2062 pour 2026) : elles comptent dans les indicateurs, pas dans le graphique (la QA5).
    pub datees_plus_tard: u32,
    /// De même, celles datées après le dernier trimestre montré (dans « Toute la période », le trimestre en cours) :
    /// elles comptent dans les indicateurs, pas dans les trimestres des taxes (la QA5, passe 2).
    pub trimestres_plus_tard: u32,
    /// Les chantiers, du plus coûteux au moins coûteux ([`CHANTIERS_MAX`]), puis ceux d'après réunis (combien, et
    /// leurs sommes), puis les factures sans chantier, à part.
    pub chantiers: Vec<LigneDeChantier>,
    pub autres_chantiers: Option<(u32, LigneDeChantier)>,
    pub sans_chantier: Option<LigneDeChantier>,
    /// Les corps de métier, du plus gros au plus petit ; les dépenses sans section, à part.
    pub sections: Vec<Groupe>,
    pub sans_section: Option<Groupe>,
    /// Les principaux fournisseurs ([`FOURNISSEURS_MAX`]) ; les autres réunis (combien, et leur somme). Un fournisseur
    /// sans nom a un nom vide.
    pub fournisseurs: Vec<Groupe>,
    pub autres_fournisseurs: Option<(u32, Groupe)>,
    /// Les trimestres de la période, du plus ancien au plus récent ([`TRIMESTRES_MAX`] au plus ; `trimestres_caches` :
    /// ceux d'avant) ; « Les 12 derniers mois » qui commencent au milieu d'un trimestre l'écartent (`trimestre_coupe` :
    /// ses sommes seraient partielles sous son nom entier). Une période d'un seul mois n'en a pas.
    pub trimestres: Vec<Trimestre>,
    pub trimestres_caches: u32,
    pub trimestre_coupe: Option<(i32, u8)>,
    /// Les noms des deux taxes des factures retenues ([`NomsDesTaxes::noms`] : « TPS » et « TVQ » d'office,
    /// « TPS/TVH » quand une case en porte deux) ; et si elles se récupèrent toutes (la TPS, la TVH, la TVQ : aucune
    /// facture n'en nomme une autre) : l'estimation de ce qui est à remettre ne se fait qu'alors.
    pub taxes: (String, String),
    pub taxes_recuperables: bool,
}

/// Le rang d'un mois « AAAA-MM… », compté depuis l'an zéro (`annee * 12 + mois - 1`) : on va d'un mois à l'autre par
/// une addition, sans calendrier.
fn rang_du_mois(d: &str) -> Option<i32> {
    let a: i32 = d.get(..4)?.parse().ok()?;
    let m: i32 = d.get(5..7)?.parse().ok()?;
    (1..=12).contains(&m).then_some(a * 12 + m - 1)
}

/// Une somme partagée en `n`, au cent, la demie loin de zéro (la règle d'EG pour l'argent).
pub fn moyenne(somme: Cents, n: u32) -> Cents {
    if n == 0 {
        return 0;
    }
    let n = i64::from(n);
    somme.signum() * ((somme.abs() * 2 + n) / (2 * n))
}

/// Du plus gros au plus petit ; à égalité, dans l'ordre des noms.
fn ranger(groupes: &mut [Groupe]) {
    groupes.sort_by(|a, b| {
        b.total.cmp(&a.total).then_with(|| a.nom.to_lowercase().cmp(&b.nom.to_lowercase())).then(a.nom.cmp(&b.nom))
    });
}

/// Le tableau de bord du registre `r` sous les filtres d'en tête, `aujourdhui` (« AAAA-MM-JJ ») pour repère : un seul
/// passage sur les factures (20 000 au plus), puis des regroupements bornés.
pub fn calculer(r: &Registre, filtres: Filtres, aujourdhui: &str) -> Bord {
    let mut b = Bord::default();
    let courant = rang_du_mois(aujourdhui).unwrap_or(0);
    let mut par_mois: HashMap<i32, Mois> = HashMap::new();
    let mut par_trimestre: HashMap<i32, Trimestre> = HashMap::new();
    let mut par_section: HashMap<String, Groupe> = HashMap::new();
    let mut par_fournisseur: HashMap<String, Groupe> = HashMap::new();
    let mut par_chantier: HashMap<Option<i64>, (Cents, u32, Cents, u32)> = HashMap::new();
    // Le nom d'un chantier : celui de sa première facture dans le registre (la règle du choix d'en tête,
    // `compta::chantiers`), quels que soient les filtres.
    let mut noms_des_chantiers: HashMap<i64, String> = HashMap::new();
    let mut noms_des_taxes = NomsDesTaxes::default();
    let (mut premier, mut dernier): (Option<i32>, Option<i32>) = (None, None);
    for f in &r.factures {
        if let Some(id) = f.projet_id {
            noms_des_chantiers.entry(id).or_insert_with(|| f.projet.clone().unwrap_or_else(|| format!("Projet {id}")));
        }
        let (depense, revenu) = (f.depense(), f.revenu());
        b.clients_inscrits |= revenu;
        if !(depense || revenu) || !filtres.retient(f, aujourdhui) {
            continue;
        }
        let rang = f.date.as_deref().and_then(rang_du_mois);
        match rang {
            Some(m) => {
                premier = Some(premier.map_or(m, |p| p.min(m)));
                dernier = Some(dernier.map_or(m, |d| d.max(m)));
            }
            None => b.sans_date += 1,
        }
        b.sans_sous_total += u32::from(f.sous_total.is_none());
        b.sans_taxe += u32::from(f.tps.is_none() || f.tvq.is_none());
        noms_des_taxes.compter(f);
        let total = f.total_signe();
        let chantier = par_chantier.entry(f.projet_id).or_insert((0, 0, 0, 0));
        let (mois, trimestre) = match rang {
            Some(m) => {
                let q = m.div_euclid(3);
                (
                    Some(par_mois.entry(m).or_insert_with(|| Mois::vide(m))),
                    Some(par_trimestre.entry(q).or_insert_with(|| Trimestre::vide(q, courant))),
                )
            }
            None => (None, None),
        };
        if depense {
            b.depenses.ajouter(f);
            b.brouillons += u32::from(f.brouillon());
            b.depenses_sans_sous_total += u32::from(f.sous_total.is_none());
            b.depenses_sans_taxe += u32::from(f.tps.is_none() || f.tvq.is_none());
            match fiabilite(f) {
                Fiabilite::Sure => b.surs += 1,
                Fiabilite::AVerifier => b.a_verifier += 1,
                Fiabilite::NonJugee => b.non_juges += 1,
            }
            chantier.0 += total;
            chantier.1 += 1;
            for (cle, groupes) in
                [(f.section.as_deref(), &mut par_section), (Some(f.tiers.as_str()), &mut par_fournisseur)]
            {
                // Le nom NORMALISÉ regroupe, comme la liste filtre et comme « Par fournisseur » (`sommaires.rs`) :
                // « Béton Fictif » et « BÉTON FICTIF » sont une seule barre, et son clic ouvre exactement ses factures
                // (la QA5, passe 1 : la barre disait 3 factures, la liste qu'elle ouvrait 6). Le nom montré : celui de
                // la première facture.
                let nom = cle.unwrap_or("").trim();
                let g = groupes
                    .entry(eve::parole::normaliser(nom))
                    .or_insert_with(|| Groupe { nom: nom.to_string(), ..Groupe::default() });
                g.total += total;
                g.n += 1;
            }
            if let Some(x) = mois {
                x.depenses += total;
                x.n_depenses += 1;
            }
            if let Some(q) = trimestre {
                q.tps_payee += f.tps_signee();
                q.tvq_payee += f.tvq_signee();
                q.n_depenses += 1;
            }
        } else {
            b.revenus.ajouter(f);
            b.encaisse += f.encaisse();
            b.reglees += u32::from(f.reglee());
            let reste = f.reste();
            if reste > 0 {
                b.a_recevoir += reste;
                b.n_a_recevoir += 1;
                if f.en_retard(aujourdhui) {
                    b.en_retard += reste;
                    b.n_en_retard += 1;
                }
            }
            chantier.2 += total;
            chantier.3 += 1;
            if let Some(x) = mois {
                x.revenus += total;
                x.n_revenus += 1;
            }
            if let Some(q) = trimestre {
                q.tps_percue += f.tps_signee();
                q.tvq_percue += f.tvq_signee();
                q.n_revenus += 1;
            }
        }
    }
    // La moyenne par mois : sur les mois qui ont des dépenses.
    let actifs: Vec<&Mois> = par_mois.values().filter(|m| m.n_depenses > 0).collect();
    b.mois_actifs = actifs.len() as u32;
    b.moyenne = moyenne(actifs.iter().map(|m| m.depenses).sum(), b.mois_actifs);
    // Les mois dessinés : ceux de la période choisie (les douze derniers, l'année entière — jusqu'à ce mois-ci pour
    // l'année en cours —, le mois), ou, pour toute la période, du premier au dernier mois qui porte une facture.
    let bornes = match filtres.periode {
        // Du premier mois qui porte une facture au dernier — jamais au-delà du mois en cours : une seule facture datée
        // loin dans l'avenir calait la fenêtre de 48 mois (et les trimestres) sur l'avenir, et le graphique se vidait
        // (la QA5, passe 1, mesuré : une facture de 2062). Elle se dit en note.
        Periode::Tout => premier.zip(dernier).map(|(p, d)| (p.min(courant), d.min(courant))),
        Periode::DouzeMois => Some((courant - 11, dernier.map_or(courant, |d| d.max(courant)))),
        Periode::Annee(a) if a == courant.div_euclid(12) => {
            Some((a * 12, dernier.map_or(courant, |d| d.max(courant)).min(a * 12 + 11)))
        }
        Periode::Annee(a) => Some((a * 12, a * 12 + 11)),
        Periode::Mois(a, m) => Some((a * 12 + i32::from(m.clamp(1, 12)) - 1, a * 12 + i32::from(m.clamp(1, 12)) - 1)),
    };
    if let Some((debut, fin)) = bornes {
        let dessine = (fin - MOIS_MAX as i32 + 1).max(debut);
        b.mois = (dessine..=fin).map(|m| par_mois.get(&m).cloned().unwrap_or_else(|| Mois::vide(m))).collect();
        b.mois_caches = (dessine - debut) as u32;
        b.datees_plus_tard = par_mois.iter().filter(|(m, _)| **m > fin).map(|(_, x)| x.n_depenses + x.n_revenus).sum();
        for (m, x) in &par_mois {
            if *m < dessine {
                b.mois_caches_avec_depenses += u32::from(x.n_depenses > 0);
                b.revenus_caches.0 += x.n_revenus;
                b.revenus_caches.1 += x.revenus;
            }
        }
        // Les trimestres : ceux de la période, entiers ; une période d'un seul mois n'en a pas.
        if !matches!(filtres.periode, Periode::Mois(..)) {
            let mut q_debut = debut.div_euclid(3);
            if filtres.periode == Periode::DouzeMois && q_debut * 3 < debut {
                b.trimestre_coupe = Some((q_debut.div_euclid(4), (q_debut.rem_euclid(4) + 1) as u8));
                q_debut += 1;
            }
            let q_fin = fin.div_euclid(3);
            if q_fin >= q_debut {
                b.trimestres_plus_tard =
                    par_trimestre.iter().filter(|(q, _)| **q > q_fin).map(|(_, t)| t.n_depenses + t.n_revenus).sum();
                let depuis = (q_fin - TRIMESTRES_MAX as i32 + 1).max(q_debut);
                b.trimestres_caches = (depuis - q_debut) as u32;
                b.trimestres = (depuis..=q_fin)
                    .map(|q| par_trimestre.get(&q).cloned().unwrap_or_else(|| Trimestre::vide(q, courant)))
                    .collect();
            }
        }
    }
    // Les chantiers : du plus coûteux au moins coûteux, ceux qui n'ont que des revenus ensuite ; les factures sans
    // chantier, à part.
    let mut chantiers: Vec<LigneDeChantier> = Vec::new();
    for (id, (dep, n_dep, rev, n_rev)) in par_chantier {
        let ligne = match id {
            Some(id) => {
                let nom = noms_des_chantiers.get(&id).cloned().unwrap_or_else(|| format!("Projet {id}"));
                LigneDeChantier::de(Chantier::Projet(id), nom)
            }
            None => LigneDeChantier::de(Chantier::Aucun, "Sans chantier".into()),
        };
        let ligne = LigneDeChantier { depenses: dep, n_depenses: n_dep, revenus: rev, n_revenus: n_rev, ..ligne };
        if id.is_some() {
            chantiers.push(ligne);
        } else {
            b.sans_chantier = Some(ligne);
        }
    }
    let id = |c: &Chantier| if let Chantier::Projet(id) = c { *id } else { i64::MAX };
    chantiers.sort_by(|a, b| {
        b.depenses
            .cmp(&a.depenses)
            .then(b.revenus.cmp(&a.revenus))
            .then_with(|| a.nom.to_lowercase().cmp(&b.nom.to_lowercase()))
            .then_with(|| id(&a.chantier).cmp(&id(&b.chantier)))
    });
    if chantiers.len() > CHANTIERS_MAX {
        let reste = chantiers.split_off(CHANTIERS_MAX);
        let mut autres = LigneDeChantier::de(Chantier::Tous, String::new());
        for l in &reste {
            autres.depenses += l.depenses;
            autres.n_depenses += l.n_depenses;
            autres.revenus += l.revenus;
            autres.n_revenus += l.n_revenus;
        }
        autres.nom =
            if reste.len() == 1 { "L'autre chantier".into() } else { format!("Les {} autres chantiers", reste.len()) };
        b.autres_chantiers = Some((reste.len() as u32, autres));
    }
    b.chantiers = chantiers;
    // Les corps de métier : la section telle qu'écrite ; sans section, à part.
    b.sans_section = par_section.remove("");
    b.sections = par_section.into_values().collect();
    ranger(&mut b.sections);
    // Les fournisseurs : les dix premiers, les autres réunis.
    let mut fournisseurs: Vec<Groupe> = par_fournisseur.into_values().collect();
    ranger(&mut fournisseurs);
    if fournisseurs.len() > FOURNISSEURS_MAX {
        let reste = fournisseurs.split_off(FOURNISSEURS_MAX);
        let autres = Groupe {
            nom: if reste.len() == 1 {
                "L'autre fournisseur".into()
            } else {
                format!("Les {} autres fournisseurs", reste.len())
            },
            total: reste.iter().map(|g| g.total).sum(),
            n: reste.iter().map(|g| g.n).sum(),
        };
        b.autres_fournisseurs = Some((reste.len() as u32, autres));
    }
    b.fournisseurs = fournisseurs;
    // Les noms des taxes, et si elles se récupèrent : la règle du contrat, sur les factures retenues.
    b.taxes = noms_des_taxes.noms();
    b.taxes_recuperables = noms_des_taxes.recuperables();
    b
}

// ---- Ce qu'il dit ---------------------------------------------------------------------------------------------------

/// « 1 facture », « 3 factures ».
pub fn pluriel(n: u32, mot: &str, mots: &str) -> String {
    // « 13 566 factures » : une espace fine entre les milliers, comme l'argent (la QA5, passe 2).
    format!("{} {}", super::par_milliers(u64::from(n)), if n > 1 { mots } else { mot })
}

/// La part de `a` dans `b`, comme EG : une décimale sous 10 %, aucune au-dessus, « < 0,1 % » pour une part infime ;
/// rien pour une part nulle ou négative, ou un tout qui ne l'est pas moins.
pub fn part(a: Cents, b: Cents) -> Option<String> {
    if a <= 0 || b <= 0 {
        return None;
    }
    let m = i128::from(a) * 1000 / i128::from(b);
    Some(match m {
        0 => "<\u{a0}0,1\u{a0}%".into(),
        m if m < 100 => format!("{},{}\u{a0}%", m / 10, m % 10),
        m => format!("{}\u{a0}%", (m + 5) / 10),
    })
}

/// Où mène un clic : une liste (avec l'état qu'elle sait filtrer — « a_recevoir », « reglees », « a_verifier »…), ou
/// une vue des sommaires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vers {
    Fournisseurs(Option<&'static str>),
    Clients(Option<&'static str>),
    ParMois,
    ParChantier,
}

/// La sorte d'un indicateur : son icône et sa teinte, au dessin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sorte {
    Depenses,
    AvantTaxes,
    TaxesPayees,
    Moyenne,
    AVerifier,
    Revenus,
    Encaisse,
    ARecevoir,
    Marge,
    TaxesPercues,
    TaxesNettes,
}

/// Un indicateur, en tuile : son titre, sa valeur, son détail ; en alerte (un retard, une marge négative) ; où mène
/// un clic, et ce que dit sa bulle.
#[derive(Clone, Debug, PartialEq)]
pub struct Indicateur {
    pub sorte: Sorte,
    pub titre: String,
    pub valeur: String,
    pub detail: String,
    pub alerte: bool,
    pub vers: Option<Vers>,
    pub bulle: &'static str,
}

fn indicateur(sorte: Sorte, titre: &str, valeur: String, detail: String) -> Indicateur {
    Indicateur { sorte, titre: titre.into(), valeur, detail, alerte: false, vers: None, bulle: "" }
}

/// « 1 234,50 $ · 2 345,00 $ » des deux taxes, nommées.
fn deux_taxes(b: &Bord, tps: Cents, tvq: Cents) -> String {
    if b.taxes.1.is_empty() {
        return format!("{} {}", b.taxes.0, argent(tps + tvq));
    }
    format!("{} {} · {} {}", b.taxes.0, argent(tps), b.taxes.1, argent(tvq))
}

impl Bord {
    /// Les noms des taxes, liés par `et` ou `ou` : « TPS et TVQ », ou le seul nom d'une taxe unique (« TVH »).
    pub fn les_taxes(&self, liaison: &str) -> String {
        if self.taxes.1.is_empty() {
            self.taxes.0.clone()
        } else {
            format!("{} {liaison} {}", self.taxes.0, self.taxes.1)
        }
    }
}

/// Les indicateurs des dépenses : Dépenses, Avant taxes, Taxes payées, Moyenne par mois, À vérifier.
pub fn indicateurs_des_depenses(b: &Bord) -> Vec<Indicateur> {
    let d = &b.depenses;
    let mut detail = pluriel(d.n, "facture", "factures");
    if b.brouillons > 0 {
        detail += &format!(" · dont {} en brouillon", b.brouillons);
    }
    if d.sans_total > 0 {
        detail += &format!(" · {} sans total", d.sans_total);
    }
    let depenses = Indicateur {
        vers: Some(Vers::Fournisseurs(None)),
        bulle: "Voir les factures des fournisseurs",
        ..indicateur(Sorte::Depenses, "Dépenses", argent(d.total), detail)
    };
    let avant = indicateur(Sorte::AvantTaxes, "Avant taxes", argent(d.sous_total), deux_taxes(b, d.tps, d.tvq));
    let payees = d.tps + d.tvq;
    let taxes = indicateur(
        Sorte::TaxesPayees,
        "Taxes payées",
        argent(payees),
        part(payees, d.total).map_or_else(|| b.les_taxes("et"), |p| format!("{p} des dépenses")),
    );
    let sur = if b.mois_actifs > 0 {
        // « sur 1 mois qui a des dépenses », « sur 2 mois qui ont des dépenses » (la QA5 : l'accord).
        let verbe = if b.mois_actifs > 1 { "ont" } else { "a" };
        format!("sur {} qui {verbe} des dépenses", pluriel(b.mois_actifs, "mois", "mois"))
    } else {
        "aucune dépense datée".into()
    };
    let moyenne = Indicateur {
        vers: Some(Vers::ParMois),
        bulle: "Voir les dépenses mois par mois",
        ..indicateur(Sorte::Moyenne, "Moyenne par mois", argent(b.moyenne), sur)
    };
    let detail = match (b.a_verifier, part(i64::from(b.a_verifier), i64::from(d.n))) {
        (0, _) => "aucune facture reçue à vérifier".to_string(),
        (_, Some(p)) => format!("factures reçues à vérifier ({p})"),
        _ => "factures reçues à vérifier".to_string(),
    };
    let a_verifier = Indicateur {
        vers: (b.a_verifier > 0).then_some(Vers::Fournisseurs(Some("a_verifier"))),
        bulle: "Voir les factures dont la saisie est à vérifier",
        ..indicateur(Sorte::AVerifier, "À vérifier", format!("{} / {}", b.a_verifier, d.n), detail)
    };
    vec![depenses, avant, taxes, moyenne, a_verifier]
}

/// Les indicateurs des revenus : Revenus, Encaissé, À recevoir (ou Crédit à appliquer), Marge, Taxes perçues, Taxes
/// nettes ; aucun sans facture client au registre (le mot d'EG, [`SANS_CLIENTS`], les remplace). `periode` : une
/// période choisie se dit (l'encaissé et ce qui reste suivent la date de la facture, comme le reste).
pub fn indicateurs_des_revenus(b: &Bord, periode: Periode) -> Vec<Indicateur> {
    if !b.clients_inscrits {
        return Vec::new();
    }
    let r = &b.revenus;
    let de_la_periode = if periode == Periode::Tout { "" } else { " de la période" };
    let rien = "aucune facture client ici".to_string();
    let detail = if r.n > 0 {
        let sans = if r.sans_total > 0 { format!(" · {} sans total", r.sans_total) } else { String::new() };
        format!("{}{sans} · avant taxes {}", pluriel(r.n, "facture", "factures"), argent(r.sous_total))
    } else {
        rien.clone()
    };
    let revenus = Indicateur {
        vers: Some(Vers::Clients(None)),
        bulle: "Voir les factures des clients",
        ..indicateur(Sorte::Revenus, "Revenus", argent(r.total), detail)
    };
    let detail = if r.n == 0 {
        rien.clone()
    } else {
        let reglees = format!("{}{de_la_periode}", pluriel(b.reglees, "facture réglée", "factures réglées"));
        let partie = (r.total > 0 && (0..=r.total).contains(&b.encaisse)).then(|| part(b.encaisse, r.total)).flatten();
        match partie {
            Some(p) => format!("{p} du facturé · {reglees}"),
            None => reglees,
        }
    };
    let encaisse = Indicateur {
        // Le clic ouvre TOUTES les factures des clients : le pied de leur liste dit le même encaissé (les paiements
        // partiels compris). Il ouvrait les seules réglées, dont le pied disait moins (la QA5, passe 1).
        vers: (r.n > 0).then_some(Vers::Clients(None)),
        bulle: "Voir les factures des clients : leur pied dit l'encaissé",
        ..indicateur(Sorte::Encaisse, "Encaissé", argent(b.encaisse), detail)
    };
    let a_recevoir = a_recevoir(b, de_la_periode);
    let marge = r.sous_total - b.depenses.sous_total;
    let mut detail = "revenus − dépenses, avant taxes".to_string();
    if b.sans_sous_total > 0 {
        detail += &format!(" · {} sans sous-total", pluriel(b.sans_sous_total, "facture", "factures"));
    }
    let marge = Indicateur {
        alerte: marge < 0,
        vers: Some(Vers::ParChantier),
        bulle: "Voir la marge de chaque chantier",
        ..indicateur(Sorte::Marge, "Marge", argent(marge), detail)
    };
    let percues = Indicateur {
        vers: (r.n > 0).then_some(Vers::Clients(None)),
        bulle: "Voir les factures des clients",
        ..indicateur(Sorte::TaxesPercues, "Taxes perçues", argent(r.tps + r.tvq), deux_taxes(b, r.tps, r.tvq))
    };
    let (tps, tvq) = (r.tps - b.depenses.tps, r.tvq - b.depenses.tvq);
    // « À remettre » et « en faveur de l'entreprise » ne se concluent que pour des taxes qui se récupèrent (la TPS, la
    // TVH, la TVQ) : la taxe de vente d'un État américain ou la TVP payées à un fournisseur ne reviennent pas (la QA5,
    // passe 1 ; la TVH se récupère comme la TPS, passe 2).
    let sens = if !b.taxes_recuperables {
        "perçues − payées"
    } else if tps + tvq < 0 {
        "en faveur de l'entreprise"
    } else {
        "à remettre"
    };
    let mut detail = format!("{sens} : {}", deux_taxes(b, tps, tvq));
    if b.sans_taxe > 0 {
        detail += &format!(" · {} sans {}", b.sans_taxe, b.les_taxes("ou"));
    }
    let nettes = indicateur(Sorte::TaxesNettes, "Taxes nettes", argent(tps + tvq), detail);
    vec![revenus, encaisse, a_recevoir, marge, percues, nettes]
}

/// À recevoir : ce qui reste, et ce qui en est en retard ; un reste négatif (des notes de crédit qui l'emporteraient)
/// se dit « Crédit à appliquer », jamais « À recevoir −… ».
fn a_recevoir(b: &Bord, de_la_periode: &str) -> Indicateur {
    if b.a_recevoir < 0 {
        return Indicateur {
            vers: Some(Vers::Clients(Some("a_recevoir"))),
            bulle: "Voir les factures des clients à recevoir",
            ..indicateur(
                Sorte::ARecevoir,
                "Crédit à appliquer",
                argent(-b.a_recevoir),
                format!("des notes de crédit{de_la_periode}, plus que ce qui reste à recevoir"),
            )
        };
    }
    let detail = if b.n_a_recevoir == 0 {
        format!("rien à recevoir{}", if de_la_periode.is_empty() { "" } else { " sur les factures de la période" })
    } else if b.n_en_retard > 0 {
        format!(
            "{}{de_la_periode} · {} en retard ({})",
            pluriel(b.n_a_recevoir, "facture", "factures"),
            b.n_en_retard,
            argent(b.en_retard)
        )
    } else {
        format!("{}{de_la_periode} · aucune en retard", pluriel(b.n_a_recevoir, "facture", "factures"))
    };
    Indicateur {
        alerte: b.n_en_retard > 0,
        vers: (b.n_a_recevoir > 0).then_some(Vers::Clients(Some("a_recevoir"))),
        bulle: "Voir les factures des clients à recevoir",
        ..indicateur(Sorte::ARecevoir, "À recevoir", argent(b.a_recevoir), detail)
    }
}

/// « de mars 2026 », « d'août 2026 » (l'élision devant une voyelle) ; `majuscule` : en tête de phrase.
fn de_mois(nom: &str, majuscule: bool) -> String {
    let d = if majuscule { "D" } else { "d" };
    if nom.starts_with(['a', 'o', 'e', 'i', 'u']) { format!("{d}'{nom}") } else { format!("{d}e {nom}") }
}

/// Ce que dit le graphique par mois sous son titre : la période dessinée.
pub fn sous_titre_des_mois(b: &Bord, periode: Periode) -> String {
    match periode {
        Periode::DouzeMois => "Les 12 derniers mois.".into(),
        Periode::Annee(a) => format!("Année {a}."),
        Periode::Mois(..) => {
            let nom = b.mois.first().map(Mois::nom).unwrap_or_default();
            let mut c = nom.chars();
            c.next().map_or_else(String::new, |p| format!("{}{}.", p.to_uppercase(), c.as_str()))
        }
        Periode::Tout => match (b.mois.first(), b.mois.last()) {
            (Some(p), Some(d)) if b.mois_caches == 0 => format!("{} à {}.", de_mois(&p.nom(), true), d.nom()),
            (Some(p), Some(d)) => {
                format!("Les {} derniers mois, {} à {}.", b.mois.len(), de_mois(&p.nom(), false), d.nom())
            }
            _ => String::new(),
        },
    }
}

/// Les notes du graphique par mois : les mois trop anciens pour être dessinés (et s'ils comptent dans la moyenne), les
/// factures sans date.
pub fn notes_des_mois(b: &Bord) -> Vec<String> {
    let mut notes = Vec::new();
    if b.mois_caches > 0 {
        let mut n = if b.mois_caches == 1 {
            "Le mois le plus ancien n'est pas dessiné".to_string()
        } else {
            format!("Les {} mois les plus anciens ne sont pas dessinés", b.mois_caches)
        };
        let (n_rev, rev) = b.revenus_caches;
        if n_rev > 0 {
            n += &format!(
                ", ni leurs revenus ({}, {})",
                pluriel(n_rev, "facture client", "factures clients"),
                argent(rev)
            );
        }
        n += &match b.mois_caches_avec_depenses {
            0 => ".".to_string(),
            k if k == b.mois_caches => " ; leurs dépenses comptent dans la moyenne.".to_string(),
            k => format!(" ; les dépenses de {} d'entre eux comptent dans la moyenne.", pluriel(k, "mois", "mois")),
        };
        notes.push(n);
    }
    if b.sans_date > 0 {
        let s = if b.sans_date > 1 { "s" } else { "" };
        notes.push(format!("{} sans date, non comptée{s} ici.", pluriel(b.sans_date, "facture", "factures")));
    }
    if b.datees_plus_tard > 0 {
        let s = if b.datees_plus_tard > 1 { "s" } else { "" };
        notes.push(format!(
            "{} datée{s} après ce mois-ci : comptée{s} dans les indicateurs, pas dans ce graphique.",
            pluriel(b.datees_plus_tard, "facture", "factures")
        ));
    }
    notes
}

/// La lecture du dernier mois qui porte une facture : « Dernier mois : octobre 2026 — dépenses 344,93 $ · 3 factures ;
/// revenus 574,87 $ · 2 factures clients ».
pub fn dernier_mois(b: &Bord) -> Option<String> {
    let m = b.mois.iter().rev().find(|m| m.n_depenses > 0 || m.n_revenus > 0)?;
    let depenses = format!("dépenses {} · {}", argent(m.depenses), pluriel(m.n_depenses, "facture", "factures"));
    Some(if b.clients_inscrits {
        format!(
            "Dernier mois : {} — {depenses} ; revenus {} · {}",
            m.nom(),
            argent(m.revenus),
            pluriel(m.n_revenus, "facture client", "factures clients")
        )
    } else {
        format!("Dernier mois : {} — {depenses}", m.nom())
    })
}

/// Les notes des trimestres montrés, sous les taxes payées comme sous l'estimation : un trimestre écarté, des
/// trimestres trop anciens, des factures datées après le dernier.
pub fn notes_des_trimestres(b: &Bord) -> Vec<String> {
    let mut notes = Vec::new();
    if let (Some((a, t)), Some(premier)) = (b.trimestre_coupe, b.mois.first()) {
        notes.push(format!(
            "Le T{t} {a} n'y est pas : « 12 derniers mois » n'en prennent qu'une partie (depuis {}). Choisis \
             l'année pour le voir entier.",
            premier.nom()
        ));
    }
    if b.trimestres_caches > 0 {
        notes.push(if b.trimestres_caches == 1 {
            "Le trimestre le plus ancien n'est pas montré.".to_string()
        } else {
            format!("Les {} trimestres les plus anciens ne sont pas montrés.", b.trimestres_caches)
        });
    }
    // Dans « Toute la période », les trimestres s'arrêtent à celui d'aujourd'hui (une faute de frappe, 2062, ne cale
    // plus la fenêtre sur l'avenir) : les factures d'après comptent dans les tuiles, pas ici — dit, sinon la somme des
    // trimestres et la tuile « Taxes nettes » divergeaient en silence (la QA5, passe 2).
    if b.trimestres_plus_tard > 0 {
        let s = if b.trimestres_plus_tard > 1 { "s" } else { "" };
        notes.push(format!(
            "{} datée{s} après ce trimestre-ci : comptée{s} dans les indicateurs, pas dans ces trimestres.",
            pluriel(b.trimestres_plus_tard, "facture", "factures")
        ));
    }
    notes
}

/// Les notes de l'estimation des taxes à remettre : un chantier choisi (une déclaration se fait pour toute
/// l'entreprise), celles des trimestres ([`notes_des_trimestres`]), des taxes manquantes ; puis ce qu'elle est.
pub fn notes_de_la_declaration(b: &Bord, filtres: Filtres) -> Vec<String> {
    let mut notes = Vec::new();
    if filtres.chantier != Chantier::Tous {
        notes.push(
            "Pour ce chantier seulement : une déclaration se fait pour toute l'entreprise (« Tous les chantiers »)."
                .to_string(),
        );
    }
    notes.extend(notes_des_trimestres(b));
    if b.sans_taxe > 0 {
        let s = if b.sans_taxe > 1 { "nt" } else { "" };
        // « sans TPS ou TVQ », ou le seul nom d'une taxe unique : jamais « sans TPS ou  » (la QA5, passe 2).
        notes.push(format!(
            "{} sans {} y compte{s} pour 0 : à compléter.",
            pluriel(b.sans_taxe, "facture", "factures"),
            b.les_taxes("ou")
        ));
    }
    notes.push(
        "Une estimation d'après les factures lues dans Constructo AI, à la date de chaque facture : une nette positive \
         est à remettre, une négative revient à l'entreprise. La déclaration officielle reste celle de l'ERP, d'après son grand \
         livre."
            .to_string(),
    );
    notes
}

#[cfg(test)]
mod essais {
    use super::*;
    use crate::compta::Cote;

    /// Une facture écrite à la main : son total est la somme de ses montants (aucun s'il manque son sous-total), son
    /// solde ce qui reste après `paye`.
    #[allow(clippy::too_many_arguments)]
    fn facture(
        id: i64,
        cote: Cote,
        statut: &str,
        type_document: &str,
        date: Option<&str>,
        tiers: &str,
        projet: Option<(i64, &str)>,
        section: Option<&str>,
        fiabilite: Option<&str>,
        montants: Option<(Cents, Cents, Cents)>,
        paye: Cents,
    ) -> Facture {
        let total = montants.map(|(st, tps, tvq)| st + tps + tvq);
        Facture {
            id,
            numero: format!("F-{id}"),
            cote,
            type_document: type_document.into(),
            statut: statut.into(),
            date: date.map(String::from),
            tiers: tiers.into(),
            projet_id: projet.map(|p| p.0),
            projet: projet.map(|p| p.1.to_string()),
            section: section.map(String::from),
            fiabilite: fiabilite.map(String::from),
            sous_total: montants.map(|m| m.0),
            tps: montants.map(|m| m.1),
            tvq: montants.map(|m| m.2),
            total,
            paye: Some(paye),
            solde: total.map(|t| t - paye),
            taxe1: Some("TPS".into()),
            taxe2: Some("TVQ".into()),
            ..Facture::default()
        }
    }

    const AUJOURDHUI: &str = "2026-11-02";
    const ENTREPOT: Option<(i64, &str)> = Some((7, "Entrepôt"));
    const ECOLE: Option<(i64, &str)> = Some((9, "École"));

    /// Le petit registre des essais, écrit à la main. Ses factures reçues (les dépenses) :
    /// - 1 : Béton A, Entrepôt, Fondation, sûre, août, 1 000,00 $ + 50,00 + 99,75 = 1 149,75 $, payée ;
    /// - 2 : Bois B, Entrepôt, Structure, à vérifier, août, 2 000,00 + 100,00 + 199,50 = 2 299,50 $ ;
    /// - 3 : Béton A, École, Fondation, non jugée, octobre, un BROUILLON de 500,00 + 25,00 + 49,88 = 574,88 $ ;
    /// - 4 : Bois B, une NOTE DE CRÉDIT, Entrepôt, Structure, sûre, octobre, 200,00 + 10,00 + 19,95 = 229,95 $ ;
    /// - 5 : Toit C, SANS TOTAL, SANS CHANTIER, sans section, sûre, octobre ;
    /// - 6 : Toit C, ANNULÉE, École, septembre ; 7 : Toit C, PRO FORMA, École, septembre (ni l'une ni l'autre ne
    ///   compte) ;
    /// - 8 : Divers D, SANS DATE, sans chantier, Divers, à vérifier, 100,00 + 5,00 + 9,98 = 114,98 $.
    ///
    /// Ses factures aux clients (les revenus) :
    /// - 11 : Client X, Entrepôt, août, 4 000,00 + 200,00 + 399,00 = 4 599,00 $, payée ;
    /// - 12 : Client X, Entrepôt, septembre, 3 000,00 + 150,00 + 299,25 = 3 449,25 $, 1 000,00 $ payés, échue le
    ///   25 octobre (en retard le 2 novembre) ;
    /// - 13 : Client Y, École, 15 octobre, 1 000,00 + 50,00 + 99,75 = 1 149,75 $, sans échéance (le 14 novembre) ;
    /// - 14 : Client X, une NOTE DE CRÉDIT, Entrepôt, octobre, 500,00 + 25,00 + 49,88 = 574,88 $, dont 100,00 $
    ///   remboursés au client ;
    /// - 15 : un BROUILLON, 16 : une PRO FORMA, 17 : une ANNULÉE (aucune ne compte).
    ///
    /// Deux trimestres : le T3 2026 (août, septembre) et le T4 2026 (octobre, celui d'aujourd'hui).
    fn registre() -> Registre {
        use Cote::{Client, Fournisseur};
        let mut f = vec![
            facture(
                1,
                Fournisseur,
                "PAYEE",
                "FACTURE",
                Some("2026-08-10"),
                "Béton A",
                ENTREPOT,
                Some("2. Fondation"),
                Some("sur"),
                Some((100_000, 5_000, 9_975)),
                114_975,
            ),
            facture(
                2,
                Fournisseur,
                "ENVOYEE",
                "FACTURE",
                Some("2026-08-20"),
                "Bois B",
                ENTREPOT,
                Some("3. Structure"),
                Some("a_verifier"),
                Some((200_000, 10_000, 19_950)),
                0,
            ),
            facture(
                3,
                Fournisseur,
                "BROUILLON",
                "FACTURE",
                Some("2026-10-05"),
                "Béton A",
                ECOLE,
                Some("2. Fondation"),
                None,
                Some((50_000, 2_500, 4_988)),
                0,
            ),
            facture(
                4,
                Fournisseur,
                "ENVOYEE",
                "AVOIR",
                Some("2026-10-12"),
                "Bois B",
                ENTREPOT,
                Some("3. Structure"),
                Some("sur"),
                Some((20_000, 1_000, 1_995)),
                0,
            ),
            facture(
                5,
                Fournisseur,
                "ENVOYEE",
                "FACTURE",
                Some("2026-10-20"),
                "Toit C",
                None,
                None,
                Some("sur"),
                None,
                0,
            ),
            facture(
                6,
                Fournisseur,
                "ANNULEE",
                "FACTURE",
                Some("2026-09-01"),
                "Toit C",
                ECOLE,
                Some("4. Toiture"),
                Some("sur"),
                Some((70_000, 3_500, 6_983)),
                0,
            ),
            facture(
                7,
                Fournisseur,
                "ENVOYEE",
                "PROFORMA",
                Some("2026-09-02"),
                "Toit C",
                ECOLE,
                Some("4. Toiture"),
                Some("sur"),
                Some((70_000, 3_500, 6_983)),
                0,
            ),
            facture(
                8,
                Fournisseur,
                "ENVOYEE",
                "FACTURE",
                None,
                "Divers D",
                None,
                Some("12. Divers"),
                Some("a_verifier"),
                Some((10_000, 500, 998)),
                0,
            ),
            facture(
                11,
                Client,
                "PAYEE",
                "FACTURE",
                Some("2026-08-25"),
                "Client X",
                ENTREPOT,
                None,
                None,
                Some((400_000, 20_000, 39_900)),
                459_900,
            ),
            facture(
                12,
                Client,
                "PARTIELLEMENT_PAYEE",
                "FACTURE",
                Some("2026-09-25"),
                "Client X",
                ENTREPOT,
                None,
                None,
                Some((300_000, 15_000, 29_925)),
                100_000,
            ),
            facture(
                13,
                Client,
                "ENVOYEE",
                "FACTURE",
                Some("2026-10-15"),
                "Client Y",
                ECOLE,
                None,
                None,
                Some((100_000, 5_000, 9_975)),
                0,
            ),
            facture(
                14,
                Client,
                "ENVOYEE",
                "AVOIR",
                Some("2026-10-28"),
                "Client X",
                ENTREPOT,
                None,
                None,
                Some((50_000, 2_500, 4_988)),
                10_000,
            ),
            facture(
                15,
                Client,
                "BROUILLON",
                "FACTURE",
                Some("2026-10-30"),
                "Client Z",
                ECOLE,
                None,
                None,
                Some((80_000, 4_000, 7_980)),
                0,
            ),
            facture(
                16,
                Client,
                "ENVOYEE",
                "PRO_FORMA",
                Some("2026-10-30"),
                "Client Z",
                ECOLE,
                None,
                None,
                Some((90_000, 4_500, 8_978)),
                0,
            ),
            facture(
                17,
                Client,
                "ANNULEE",
                "FACTURE",
                Some("2026-10-31"),
                "Client Z",
                ECOLE,
                None,
                None,
                Some((60_000, 3_000, 5_985)),
                0,
            ),
        ];
        // La facture 12 : échue le 25 octobre.
        f[9].echeance = Some("2026-10-25".into());
        Registre { au_serveur: f.len() as u64, factures: f, ..Registre::default() }
    }

    fn tout() -> Filtres {
        Filtres::default()
    }

    fn mois(annee: i32, mois: u8, depenses: Cents, n_depenses: u32, revenus: Cents, n_revenus: u32) -> Mois {
        Mois { annee, mois, depenses, n_depenses, revenus, n_revenus }
    }

    #[test]
    fn les_indicateurs_des_depenses() {
        let b = calculer(&registre(), tout(), AUJOURDHUI);
        // Six dépenses (1, 2, 3, 4, 5, 8) : l'annulée et la pro forma n'y sont pas ; la note de crédit se retranche ;
        // la facture sans total compte pour 0.
        // Sous-totaux : 1 000 + 2 000 + 500 − 200 + 0 + 100 = 3 400,00 $ ; TPS : 50 + 100 + 25 − 10 + 5 = 170,00 $ ;
        // TVQ : 99,75 + 199,50 + 49,88 − 19,95 + 9,98 = 339,16 $ ; total : 3 909,16 $.
        let d = b.depenses;
        assert_eq!((d.n, d.sous_total, d.tps, d.tvq, d.total, d.sans_total), (6, 340_000, 17_000, 33_916, 390_916, 1));
        assert_eq!(b.brouillons, 1);
        // La fiabilité : sûres 1, 4, 5 ; à vérifier 2, 8 ; non jugée 3.
        assert_eq!((b.surs, b.a_verifier, b.non_juges), (3, 2, 1));
        // La moyenne : août (1 149,75 + 2 299,50) et octobre (574,88 − 229,95 + 0), les deux mois qui ont des
        // dépenses : 3 794,18 $ / 2 = 1 897,09 $ (la facture sans date n'a pas de mois).
        assert_eq!((b.mois_actifs, b.moyenne), (2, 189_709));
        let i = indicateurs_des_depenses(&b);
        let dits: Vec<(&str, &str, &str)> =
            i.iter().map(|x| (x.titre.as_str(), x.valeur.as_str(), x.detail.as_str())).collect();
        assert_eq!(
            dits,
            [
                ("Dépenses", "3\u{202f}909,16\u{a0}$", "6 factures · dont 1 en brouillon · 1 sans total"),
                ("Avant taxes", "3\u{202f}400,00\u{a0}$", "TPS 170,00\u{a0}$ · TVQ 339,16\u{a0}$"),
                // 509,16 / 3 909,16 = 13,02 % : « 13 % ».
                ("Taxes payées", "509,16\u{a0}$", "13\u{a0}% des dépenses"),
                ("Moyenne par mois", "1\u{202f}897,09\u{a0}$", "sur 2 mois qui ont des dépenses"),
                ("À vérifier", "2 / 6", "factures reçues à vérifier (33\u{a0}%)"),
            ]
        );
        assert_eq!(i[0].vers, Some(Vers::Fournisseurs(None)));
        assert_eq!(i[4].vers, Some(Vers::Fournisseurs(Some("a_verifier"))));
        assert_eq!((i[1].vers, i[2].vers, i[3].vers), (None, None, Some(Vers::ParMois)));
    }

    /// La QA5 (passe 1) : « Béton Fictif ltée » et « BÉTON  FICTIF LTÉE » faisaient deux barres, et le clic sur l'une
    /// ouvrait une liste (qui filtre par le nom plié) aux factures des deux.
    #[test]
    fn un_fournisseur_et_une_section_se_regroupent_par_leur_nom_plie() {
        use Cote::Fournisseur;
        let m = Some((10_000, 500, 998));
        let r = Registre {
            factures: vec![
                facture(
                    1,
                    Fournisseur,
                    "PAYEE",
                    "FACTURE",
                    Some("2026-10-01"),
                    "Béton Fictif ltée",
                    None,
                    Some("2. Fondation"),
                    Some("sur"),
                    m,
                    11_498,
                ),
                facture(
                    2,
                    Fournisseur,
                    "PAYEE",
                    "FACTURE",
                    Some("2026-10-02"),
                    "BÉTON  FICTIF LTÉE",
                    None,
                    Some("2.  fondation"),
                    Some("sur"),
                    m,
                    11_498,
                ),
            ],
            ..Registre::default()
        };
        let b = calculer(&r, tout(), AUJOURDHUI);
        let f = &b.fournisseurs;
        assert_eq!(f.len(), 1, "une seule barre");
        // 2 × (100,00 + 5,00 + 9,98) = 229,96 $ ; nommé comme sa première facture.
        assert_eq!((f[0].nom.as_str(), f[0].n, f[0].total), ("Béton Fictif ltée", 2, 22_996));
        assert_eq!((b.sections.len(), b.sections[0].nom.as_str(), b.sections[0].n), (1, "2. Fondation", 2));
    }

    /// La QA5 (passe 1) : hors du Québec, la tuile concluait « à remettre » ou « en votre faveur » sur une taxe qui ne
    /// se récupère pas, et nommait une « TVQ 0,00 $ » inventée. La passe 2 : la TVH, elle, se récupère comme la TPS.
    #[test]
    fn une_taxe_qui_ne_se_recupere_pas_ne_conclut_pas() {
        use Cote::{Client, Fournisseur};
        let recue = facture(
            1,
            Fournisseur,
            "PAYEE",
            "FACTURE",
            Some("2026-10-01"),
            "Fournisseur A",
            None,
            None,
            None,
            Some((10_000, 1_300, 0)),
            11_300,
        );
        let emise = facture(
            2,
            Client,
            "ENVOYEE",
            "FACTURE",
            Some("2026-10-05"),
            "Client B",
            None,
            None,
            None,
            Some((20_000, 2_600, 0)),
            0,
        );
        let nettes = |taxe: &str| {
            let (mut recue, mut emise) = (recue.clone(), emise.clone());
            for f in [&mut recue, &mut emise] {
                f.taxe1 = Some(taxe.into());
                f.taxe2 = None;
            }
            let b = calculer(&Registre { factures: vec![recue, emise], ..Registre::default() }, tout(), AUJOURDHUI);
            assert_eq!(b.taxes, (taxe.to_string(), String::new()));
            let i = indicateurs_des_revenus(&b, Periode::Tout);
            let nettes = i.iter().find(|x| x.titre == "Taxes nettes").expect("la tuile des taxes nettes").clone();
            (b.taxes_recuperables, nettes.detail)
        };
        // 26,00 perçus − 13,00 payés = 13,00 $ : la taxe de vente d'un État ne conclut pas ; la TVH, si.
        assert_eq!(nettes("Sales Tax"), (false, "perçues − payées : Sales Tax 13,00\u{a0}$".to_string()));
        assert_eq!(nettes("TVH"), (true, "à remettre : TVH 13,00\u{a0}$".to_string()));
        assert_eq!(nettes("HST"), (true, "à remettre : HST 13,00\u{a0}$".to_string()));
    }

    /// La QA5 (passe 1, mutation) : une facture datée 2062-10-15 (une faute de frappe) calait la fenêtre de 48 mois et
    /// les trimestres sur l'avenir ; le graphique ne montrait plus qu'un mois de dépenses.
    #[test]
    fn une_facture_datee_loin_dans_l_avenir_ne_vide_pas_le_graphique() {
        use Cote::Fournisseur;
        let mut factures: Vec<Facture> = (1..=10)
            .map(|m| {
                let date = format!("2026-{m:02}-15");
                facture(
                    m,
                    Fournisseur,
                    "PAYEE",
                    "FACTURE",
                    Some(date.as_str()),
                    "A",
                    None,
                    None,
                    None,
                    Some((10_000, 0, 0)),
                    10_000,
                )
            })
            .collect();
        factures.push(facture(
            11,
            Fournisseur,
            "ENVOYEE",
            "FACTURE",
            Some("2062-10-15"),
            "A",
            None,
            None,
            None,
            Some((10_000, 0, 0)),
            0,
        ));
        let b = calculer(&Registre { factures, ..Registre::default() }, tout(), AUJOURDHUI);
        // La fenêtre va de janvier 2026 au mois en cours (novembre) : onze mois, aucun caché ; dix ont des dépenses.
        assert_eq!((b.mois.len(), b.mois_caches), (11, 0));
        assert_eq!(
            (b.mois.first().map(Mois::cle), b.mois.last().map(Mois::cle)),
            (Some("2026-01".into()), Some("2026-11".into()))
        );
        assert_eq!(b.mois.iter().filter(|m| m.depenses > 0).count(), 10);
        // Elle compte dans les indicateurs (11 factures, 1 100,00 $), et se dit en note.
        assert_eq!((b.depenses.n, b.depenses.total, b.datees_plus_tard), (11, 110_000, 1));
        assert!(
            notes_des_mois(&b).iter().any(|n| n.starts_with("1 facture datée après ce mois-ci")),
            "{:?}",
            notes_des_mois(&b)
        );
        // Les trimestres finissent au trimestre en cours.
        assert_eq!(b.trimestres.last().map(Trimestre::court).as_deref(), Some("T4 2026"));
    }

    /// La QA5 (passe 2) : dans « Toute la période », une facture datée après le trimestre en cours comptait dans la
    /// tuile « Taxes nettes » et pas dans les trimestres, sans que rien le dise (3 144,75 $ dans la tuile, 1 647,25 $
    /// dans l'estimation). Une facture datée plus tard DANS le trimestre en cours y compte, elle.
    #[test]
    fn une_facture_d_un_trimestre_a_venir_se_dit_sous_les_trimestres() {
        use Cote::Client;
        let emise = |id, date: &str| {
            facture(
                id,
                Client,
                "ENVOYEE",
                "FACTURE",
                Some(date),
                "Client",
                None,
                None,
                None,
                Some((10_000, 500, 998)),
                0,
            )
        };
        let factures =
            vec![emise(1, "2026-10-15"), emise(2, "2026-12-05"), emise(3, "2027-01-05"), emise(4, "2062-10-15")];
        let b = calculer(&Registre { factures, ..Registre::default() }, tout(), AUJOURDHUI);
        assert_eq!((b.revenus.n, b.datees_plus_tard, b.trimestres_plus_tard), (4, 3, 2));
        let t4 = b.trimestres.last().expect("le T4 2026");
        assert_eq!((t4.court(), t4.n_revenus, t4.tps_percue), ("T4 2026".to_string(), 2, 1_000));
        let attendue =
            "2 factures datées après ce trimestre-ci : comptées dans les indicateurs, pas dans ces trimestres.";
        assert!(notes_des_trimestres(&b).iter().any(|n| n == attendue), "{:?}", notes_des_trimestres(&b));
        assert!(notes_de_la_declaration(&b, tout()).iter().any(|n| n == attendue));
        // Dans l'année en cours, les trimestres vont jusqu'à la dernière facture : rien n'est laissé de côté.
        let annee = Filtres { periode: Periode::Annee(2026), ..tout() };
        let r = Registre { factures: vec![emise(1, "2026-10-15"), emise(2, "2026-12-05")], ..Registre::default() };
        assert_eq!(calculer(&r, annee, AUJOURDHUI).trimestres_plus_tard, 0);
    }

    /// La QA5 (passe 2) : une entreprise à la TPS seule (la seconde taxe vide) : la note de l'estimation disait « sans
    /// TPS ou  ».
    #[test]
    fn la_note_d_une_seule_taxe() {
        use Cote::Client;
        let mut f = facture(
            1,
            Client,
            "ENVOYEE",
            "FACTURE",
            Some("2026-10-15"),
            "Client",
            None,
            None,
            None,
            Some((10_000, 500, 0)),
            0,
        );
        (f.taxe1, f.taxe2, f.tvq) = (Some("TPS".into()), None, None);
        let b = calculer(&Registre { factures: vec![f], ..Registre::default() }, tout(), AUJOURDHUI);
        assert_eq!((b.taxes.0.as_str(), b.taxes.1.as_str(), b.taxes_recuperables, b.sans_taxe), ("TPS", "", true, 1));
        assert!(
            notes_de_la_declaration(&b, tout())
                .iter()
                .any(|n| n == "1 facture sans TPS y compte pour 0 : à compléter."),
            "{:?}",
            notes_de_la_declaration(&b, tout())
        );
    }

    /// La QA5 (passe 2) : un compte sépare ses milliers, comme l'argent (« 13 566 factures », pas « 13566 »).
    #[test]
    fn un_compte_separe_ses_milliers() {
        assert_eq!(pluriel(1, "facture", "factures"), "1 facture");
        assert_eq!(pluriel(999, "facture", "factures"), "999 factures");
        assert_eq!(pluriel(13_566, "facture", "factures"), "13\u{202f}566 factures");
    }

    #[test]
    fn les_indicateurs_des_revenus() {
        let b = calculer(&registre(), tout(), AUJOURDHUI);
        // Quatre revenus (11, 12, 13, 14) : ni le brouillon, ni la pro forma, ni l'annulée ; la note de crédit se
        // retranche. Sous-totaux : 4 000 + 3 000 + 1 000 − 500 = 7 500,00 $ ; TPS : 200 + 150 + 50 − 25 = 375,00 $ ;
        // TVQ : 399 + 299,25 + 99,75 − 49,88 = 748,12 $ ; total : 8 623,12 $.
        let r = b.revenus;
        assert_eq!((r.n, r.sous_total, r.tps, r.tvq, r.total), (4, 750_000, 37_500, 74_812, 862_312));
        // Encaissé : 4 599,00 + 1 000,00 = 5 599,00 $ (une facture réglée) ; à recevoir : 2 449,25 (la 12) + 1 149,75
        // (la 13) = 3 599,00 $, la note de crédit ne reste pas à recevoir ; en retard : la 12 seule. La note de crédit
        // n'encaisse rien : la règle du contrat (`Facture::encaisse`, montée à la fusion de l'équipe — l'ERP garde le
        // payé d'un avoir à zéro), ses 100,00 $ « remboursés » de cet essai n'y comptent pas.
        assert_eq!((b.encaisse, b.reglees), (559_900, 1));
        assert_eq!((b.a_recevoir, b.n_a_recevoir, b.en_retard, b.n_en_retard), (359_900, 2, 244_925, 1));
        assert!(b.clients_inscrits);
        let i = indicateurs_des_revenus(&b, Periode::Tout);
        let dits: Vec<(&str, &str, &str)> =
            i.iter().map(|x| (x.titre.as_str(), x.valeur.as_str(), x.detail.as_str())).collect();
        assert_eq!(
            dits,
            [
                ("Revenus", "8\u{202f}623,12\u{a0}$", "4 factures · avant taxes 7\u{202f}500,00\u{a0}$"),
                // 5 599,00 / 8 623,12 = 64,93 % : « 65 % ».
                ("Encaissé", "5\u{202f}599,00\u{a0}$", "65\u{a0}% du facturé · 1 facture réglée"),
                ("À recevoir", "3\u{202f}599,00\u{a0}$", "2 factures · 1 en retard (2\u{202f}449,25\u{a0}$)"),
                // 7 500,00 − 3 400,00 ; la facture 5 n'a pas de sous-total.
                ("Marge", "4\u{202f}100,00\u{a0}$", "revenus − dépenses, avant taxes · 1 facture sans sous-total"),
                ("Taxes perçues", "1\u{202f}123,12\u{a0}$", "TPS 375,00\u{a0}$ · TVQ 748,12\u{a0}$"),
                // 1 123,12 − 509,16 = 613,96 $ : TPS 375,00 − 170,00, TVQ 748,12 − 339,16.
                (
                    "Taxes nettes",
                    "613,96\u{a0}$",
                    "à remettre : TPS 205,00\u{a0}$ · TVQ 408,96\u{a0}$ · 1 sans TPS ou TVQ"
                ),
            ]
        );
        assert!(i[2].alerte, "une facture en retard");
        assert!(!i[3].alerte, "une marge positive");
        assert_eq!(
            i.iter().map(|x| x.vers).collect::<Vec<_>>(),
            [
                Some(Vers::Clients(None)),
                Some(Vers::Clients(None)),
                Some(Vers::Clients(Some("a_recevoir"))),
                Some(Vers::ParChantier),
                Some(Vers::Clients(None)),
                None
            ]
        );
    }

    /// Sans facture client au registre, les revenus ne se montrent pas : le mot d'EG les remplace. Avec des factures
    /// clients ailleurs (un autre chantier), ils se montrent, à zéro, et le disent.
    #[test]
    fn sans_facture_client() {
        let mut r = registre();
        r.factures.retain(|f| f.cote == Cote::Fournisseur);
        let b = calculer(&r, tout(), AUJOURDHUI);
        assert!(!b.clients_inscrits);
        assert!(indicateurs_des_revenus(&b, Periode::Tout).is_empty());
        let aucun = Filtres { chantier: Chantier::Aucun, ..tout() };
        let b = calculer(&registre(), aucun, AUJOURDHUI);
        let i = indicateurs_des_revenus(&b, Periode::Tout);
        assert_eq!((i[0].valeur.as_str(), i[0].detail.as_str()), ("0,00\u{a0}$", "aucune facture client ici"));
        assert_eq!(i[2].detail, "rien à recevoir");
        // Sans chantier : les factures 5 et 8, 0 $ et 114,98 $ ; une marge négative est en alerte.
        assert_eq!((b.depenses.n, b.depenses.total), (2, 11_498));
        assert!(i[3].alerte && i[3].valeur == "\u{2212}100,00\u{a0}$", "{:?}", i[3]);
    }

    /// Un reste négatif se dit « Crédit à appliquer » ; aucune facture à recevoir dans une période, « rien à recevoir
    /// sur les factures de la période ».
    #[test]
    fn un_credit_a_appliquer() {
        let b = Bord { clients_inscrits: true, a_recevoir: -12_345, ..Bord::default() };
        let i = a_recevoir(&b, " de la période");
        assert_eq!(
            (i.titre.as_str(), i.valeur.as_str(), i.detail.as_str()),
            (
                "Crédit à appliquer",
                "123,45\u{a0}$",
                "des notes de crédit de la période, plus que ce qui reste à recevoir"
            )
        );
        let b = Bord { clients_inscrits: true, ..Bord::default() };
        assert_eq!(a_recevoir(&b, " de la période").detail, "rien à recevoir sur les factures de la période");
    }

    #[test]
    fn les_mois_de_toute_la_periode() {
        let b = calculer(&registre(), tout(), AUJOURDHUI);
        // D'août à octobre 2026 (les mois qui portent une facture) ; septembre n'a que des revenus.
        // Août : 1 149,75 + 2 299,50 = 3 449,25 $ (2) ; revenus 4 599,00 $ (1).
        // Octobre : 574,88 − 229,95 + 0 = 344,93 $ (3) ; revenus 1 149,75 − 574,88 = 574,87 $ (2).
        assert_eq!(
            b.mois,
            [
                mois(2026, 8, 344_925, 2, 459_900, 1),
                mois(2026, 9, 0, 0, 344_925, 1),
                mois(2026, 10, 34_493, 3, 57_487, 2)
            ]
        );
        assert_eq!((b.mois_caches, b.sans_date), (0, 1));
        assert_eq!(b.mois[0].cle(), "2026-08");
        assert_eq!(sous_titre_des_mois(&b, Periode::Tout), "D'août 2026 à octobre 2026.");
        assert_eq!(notes_des_mois(&b), ["1 facture sans date, non comptée ici."]);
        assert_eq!(
            dernier_mois(&b).as_deref(),
            Some(
                "Dernier mois : octobre 2026 — dépenses 344,93\u{a0}$ · 3 factures ; revenus 574,87\u{a0}$ · 2 \
                 factures clients"
            )
        );
    }

    /// Les douze derniers mois : de décembre 2025 à novembre 2026, ceux sans facture à zéro ; la facture sans date n'y
    /// est pas ; le T4 2025 (seulement décembre) est écarté des trimestres.
    #[test]
    fn les_douze_derniers_mois_et_leurs_trimestres() {
        let douze = Filtres { periode: Periode::DouzeMois, ..tout() };
        let b = calculer(&registre(), douze, AUJOURDHUI);
        assert_eq!(b.mois.len(), 12);
        assert_eq!((b.mois[0].annee, b.mois[0].mois, b.mois[11].annee, b.mois[11].mois), (2025, 12, 2026, 11));
        assert_eq!(b.mois[8], mois(2026, 8, 344_925, 2, 459_900, 1));
        assert_eq!(b.mois[11], mois(2026, 11, 0, 0, 0, 0));
        assert_eq!((b.depenses.n, b.depenses.total, b.sans_date), (5, 379_418, 0));
        assert_eq!(b.trimestre_coupe, Some((2025, 4)));
        let noms: Vec<String> = b.trimestres.iter().map(Trimestre::court).collect();
        assert_eq!(noms, ["T1 2026", "T2 2026", "T3 2026", "T4 2026"]);
        assert_eq!(sous_titre_des_mois(&b, Periode::DouzeMois), "Les 12 derniers mois.");
        let notes = notes_de_la_declaration(&b, douze);
        assert_eq!(
            notes[0],
            "Le T4 2025 n'y est pas : « 12 derniers mois » n'en prennent qu'une partie (depuis décembre 2025). \
             Choisis l'année pour le voir entier."
        );
    }

    #[test]
    fn les_deux_trimestres() {
        let b = calculer(&registre(), tout(), AUJOURDHUI);
        // T3 2026 : payées (1, 2) TPS 50 + 100, TVQ 99,75 + 199,50 ; perçues (11, 12) TPS 200 + 150, TVQ 399 + 299,25.
        // T4 2026 : payées (3, 4, 5) TPS 25 − 10 + 0, TVQ 49,88 − 19,95 ; perçues (13, 14) TPS 50 − 25, TVQ 99,75 −
        // 49,88. La facture sans date n'a pas de trimestre.
        let t3 = Trimestre {
            annee: 2026,
            t: 3,
            tps_payee: 15_000,
            tvq_payee: 29_925,
            n_depenses: 2,
            tps_percue: 35_000,
            tvq_percue: 69_825,
            n_revenus: 2,
            etat: EtatDuTrimestre::Clos,
        };
        let t4 = Trimestre {
            annee: 2026,
            t: 4,
            tps_payee: 1_500,
            tvq_payee: 2_993,
            n_depenses: 3,
            tps_percue: 2_500,
            tvq_percue: 4_987,
            n_revenus: 2,
            etat: EtatDuTrimestre::EnCours,
        };
        assert_eq!(b.trimestres, [t3.clone(), t4.clone()]);
        assert_eq!((t3.tps_nette(), t3.tvq_nette(), t4.tps_nette(), t4.tvq_nette()), (20_000, 39_900, 1_000, 1_994));
        assert_eq!((t3.court(), t3.mois()), ("T3 2026".to_string(), "juil.–sept."));
        assert!(b.taxes_recuperables);
        assert_eq!(b.taxes, ("TPS".to_string(), "TVQ".to_string()));
        assert_eq!(b.trimestre_coupe, None);
        // Une facture d'un chantier en Ontario (la TVH) : elle se récupère comme la TPS, l'estimation se fait ; sa case
        // se nomme avec la TPS, jamais sous le seul nom « TPS » (la QA5, passe 2).
        let mut r = registre();
        r.factures[0].taxe1 = Some("TVH".into());
        let b = calculer(&r, tout(), AUJOURDHUI);
        assert!(b.taxes_recuperables);
        assert_eq!(b.taxes, ("TPS/TVH".to_string(), "TVQ".to_string()));
        // Une TVP, qui ne se récupère pas : l'estimation ne se fait plus.
        r.factures[0].taxe2 = Some("TVP".into());
        assert!(!calculer(&r, tout(), AUJOURDHUI).taxes_recuperables);
        // Sans un seul nom de taxe (une base qui ne les garde pas, une facture du MOBILE) : la TPS et la TVQ d'office,
        // et l'estimation se fait (la QA5, passe 2 : elle disparaissait).
        for f in &mut r.factures {
            (f.taxe1, f.taxe2) = (None, None);
        }
        let b = calculer(&r, tout(), AUJOURDHUI);
        assert!(b.taxes_recuperables);
        assert_eq!(b.taxes, ("TPS".to_string(), "TVQ".to_string()));
    }

    /// Par chantier : du plus coûteux au moins coûteux, « Sans chantier » à part ; par corps de métier, « Sans
    /// section » à part ; les fournisseurs, du plus gros au plus petit.
    #[test]
    fn les_chantiers_les_sections_et_les_fournisseurs() {
        let b = calculer(&registre(), tout(), AUJOURDHUI);
        // Entrepôt : 1 149,75 + 2 299,50 − 229,95 = 3 219,30 $ (3) ; revenus 4 599,00 + 3 449,25 − 574,88 = 7 473,37 $.
        // École : 574,88 $ (1, le brouillon) ; revenus 1 149,75 $. Sans chantier : 0 + 114,98 $ (2).
        let lignes: Vec<(Chantier, &str, Cents, u32, Cents, u32)> = b
            .chantiers
            .iter()
            .map(|l| (l.chantier, l.nom.as_str(), l.depenses, l.n_depenses, l.revenus, l.n_revenus))
            .collect();
        assert_eq!(
            lignes,
            [
                (Chantier::Projet(7), "Entrepôt", 321_930, 3, 747_337, 3),
                (Chantier::Projet(9), "École", 57_488, 1, 114_975, 1)
            ]
        );
        let sans = b.sans_chantier.clone().unwrap();
        assert_eq!((sans.chantier, sans.depenses, sans.n_depenses, sans.revenus), (Chantier::Aucun, 11_498, 2, 0));
        assert_eq!(b.autres_chantiers, None);
        let groupe = |nom: &str, total, n| Groupe { nom: nom.into(), total, n };
        // Structure : 2 299,50 − 229,95 ; Fondation : 1 149,75 + 574,88 ; Divers : 114,98 ; sans section : la 5.
        assert_eq!(
            b.sections,
            [groupe("3. Structure", 206_955, 2), groupe("2. Fondation", 172_463, 2), groupe("12. Divers", 11_498, 1)]
        );
        assert_eq!(b.sans_section, Some(groupe("", 0, 1)));
        assert_eq!(
            b.fournisseurs,
            [
                groupe("Bois B", 206_955, 2),
                groupe("Béton A", 172_463, 2),
                groupe("Divers D", 11_498, 1),
                groupe("Toit C", 0, 1)
            ]
        );
        assert_eq!(b.autres_fournisseurs, None);
        // Un chantier choisi : ses seules factures.
        let ecole = Filtres { chantier: Chantier::Projet(9), ..tout() };
        let b = calculer(&registre(), ecole, AUJOURDHUI);
        assert_eq!((b.depenses.n, b.depenses.total, b.brouillons, b.revenus.total), (1, 57_488, 1, 114_975));
        assert_eq!(b.mois, [mois(2026, 10, 57_488, 1, 114_975, 1)]);
        assert!(notes_de_la_declaration(&b, ecole)[0].starts_with("Pour ce chantier seulement : une déclaration"));
    }

    /// Treize fournisseurs : les dix plus gros, puis les trois autres réunis ; treize chantiers : les douze plus
    /// coûteux, puis « L'autre chantier ».
    #[test]
    fn les_autres_se_reunissent() {
        let mut factures = Vec::new();
        for k in 1..=13_i64 {
            let projet = Some((100 + k, "Chantier"));
            let mut f = facture(
                k,
                Cote::Fournisseur,
                "ENVOYEE",
                "FACTURE",
                Some("2026-10-01"),
                &format!("F{k:02}"),
                projet,
                Some("1. Excavation"),
                Some("sur"),
                Some((k * 1_000, 0, 0)),
                0,
            );
            f.projet = Some(format!("Chantier {k:02}"));
            factures.push(f);
        }
        // Deux factures du même fournisseur F01 : 1 000 + 50 000.
        factures.push(facture(
            99,
            Cote::Fournisseur,
            "ENVOYEE",
            "FACTURE",
            Some("2026-10-02"),
            "F01",
            None,
            None,
            None,
            Some((50_000, 0, 0)),
            0,
        ));
        let r = Registre { factures, ..Registre::default() };
        let b = calculer(&r, tout(), AUJOURDHUI);
        assert_eq!(b.fournisseurs.len(), 10);
        assert_eq!((b.fournisseurs[0].nom.as_str(), b.fournisseurs[0].total, b.fournisseurs[0].n), ("F01", 51_000, 2));
        // F01 (51 000), puis F13 à F05 : dix ; F04, F03 et F02 (40,00 + 30,00 + 20,00 $) réunis.
        assert_eq!(b.fournisseurs[9].nom, "F05");
        let (n, autres) = b.autres_fournisseurs.clone().unwrap();
        assert_eq!((n, autres.nom.as_str(), autres.total, autres.n), (3, "Les 3 autres fournisseurs", 9_000, 3));
        assert_eq!(b.chantiers.len(), CHANTIERS_MAX);
        assert_eq!(b.chantiers[0].nom, "Chantier 13");
        let (n, autres) = b.autres_chantiers.clone().unwrap();
        assert_eq!(
            (n, autres.nom.as_str(), autres.depenses, autres.chantier),
            (1, "L'autre chantier", 1_000, Chantier::Tous)
        );
        assert_eq!(b.sans_chantier.map(|l| l.depenses), Some(50_000));
    }

    /// Plus de 48 mois : les plus anciens ne se dessinent pas, et le disent (leurs revenus, s'ils comptent dans la
    /// moyenne) ; la moyenne compte tous les mois qui ont des dépenses.
    #[test]
    fn quarante_huit_mois_au_plus() {
        let f = |id, cote, date: &str, st| {
            facture(id, cote, "ENVOYEE", "FACTURE", Some(date), "X", None, None, None, Some((st, 0, 0)), 0)
        };
        // Janvier 2022 (une dépense et un revenu), février 2022 (un revenu), février 2026 (une dépense) : 50 mois ;
        // les deux plus anciens ont des revenus, un seul a des dépenses.
        let r = Registre {
            factures: vec![
                f(1, Cote::Fournisseur, "2022-01-15", 100),
                f(4, Cote::Client, "2022-01-20", 3_000),
                f(2, Cote::Client, "2022-02-15", 7_000),
                f(3, Cote::Fournisseur, "2026-02-15", 1),
            ],
            ..Registre::default()
        };
        let b = calculer(&r, tout(), AUJOURDHUI);
        assert_eq!(b.mois.len(), MOIS_MAX);
        assert_eq!((b.mois[0].annee, b.mois[0].mois), (2022, 3));
        assert_eq!((b.mois_caches, b.mois_caches_avec_depenses, b.revenus_caches), (2, 1, (2, 10_000)));
        // 1,00 $ et 0,01 $ sur deux mois : 0,505 $, la demie loin de zéro.
        assert_eq!((b.mois_actifs, b.moyenne), (2, 51));
        assert_eq!(
            notes_des_mois(&b),
            [
                "Les 2 mois les plus anciens ne sont pas dessinés, ni leurs revenus (2 factures clients, 100,00\u{a0}$) ; \
              les dépenses de 1 mois d'entre eux comptent dans la moyenne."
            ]
        );
        assert_eq!(sous_titre_des_mois(&b, Periode::Tout), "Les 48 derniers mois, de mars 2022 à février 2026.");
        // Les trimestres : du T1 2022 au T1 2026, les seize derniers.
        assert_eq!((b.trimestres.len(), b.trimestres_caches), (TRIMESTRES_MAX, 1));
        assert_eq!(b.trimestres[0].court(), "T2 2022");
    }

    /// Une année : de janvier à décembre, ou jusqu'à ce mois-ci pour l'année en cours ; un mois : lui seul, sans
    /// trimestre.
    #[test]
    fn une_annee_et_un_mois() {
        let b = calculer(&registre(), Filtres { periode: Periode::Annee(2026), ..tout() }, AUJOURDHUI);
        assert_eq!((b.mois.len(), b.mois[10].mois), (11, 11));
        assert_eq!(b.trimestres.len(), 4);
        let b = calculer(&registre(), Filtres { periode: Periode::Annee(2025), ..tout() }, AUJOURDHUI);
        assert_eq!((b.mois.len(), b.depenses.n, b.revenus.n), (12, 0, 0));
        let b = calculer(&registre(), Filtres { periode: Periode::Mois(2026, 8), ..tout() }, AUJOURDHUI);
        assert_eq!(b.mois, [mois(2026, 8, 344_925, 2, 459_900, 1)]);
        assert!(b.trimestres.is_empty());
        assert_eq!(sous_titre_des_mois(&b, Periode::Mois(2026, 8)), "Août 2026.");
    }

    /// Une grande entreprise : 20 000 factures (`FACTURES_MAX`), 400 chantiers, 300 fournisseurs, cinq ans : chaque
    /// facture comptée une fois, les regroupements bornés. Aucune borne de temps : sous la charge du poste (quatre
    /// compilations), une mesure en debug a dépassé la seconde à chaque passe, et en release une seconde laisserait
    /// passer un passage quadratique simple. La durée se dit (`--nocapture`), sans juger.
    #[test]
    fn vingt_mille_factures() {
        let mut factures = Vec::with_capacity(crate::compta::FACTURES_MAX);
        for k in 0..crate::compta::FACTURES_MAX as i64 {
            let cote = if k % 4 == 0 { Cote::Client } else { Cote::Fournisseur };
            let date = format!("{}-{:02}-{:02}", 2022 + k % 5, 1 + k % 12, 1 + k % 28);
            let mut f = facture(
                k,
                cote,
                "ENVOYEE",
                "FACTURE",
                Some(&date),
                &format!("F{}", k % 300),
                None,
                None,
                None,
                Some((10_000, 500, 998)),
                0,
            );
            f.projet_id = Some(k % 400);
            f.section = Some(format!("{}. Section", k % 14));
            factures.push(f);
        }
        let r = Registre { factures, ..Registre::default() };
        let debut = std::time::Instant::now();
        let b = calculer(&r, tout(), "2026-12-31");
        eprintln!("vingt mille factures : {:?}", debut.elapsed());
        assert_eq!((b.depenses.n, b.revenus.n), (15_000, 5_000));
        assert_eq!(b.depenses.total, 15_000 * 11_498);
        assert_eq!((b.chantiers.len(), b.autres_chantiers.as_ref().map(|a| a.0)), (CHANTIERS_MAX, Some(388)));
        // Les fournisseurs « F0 », « F4 »… ne sont que sur des factures de clients (k % 4 == 0) : 225 en dépenses.
        assert_eq!((b.fournisseurs.len(), b.autres_fournisseurs.as_ref().map(|a| a.0)), (FOURNISSEURS_MAX, Some(215)));
        assert_eq!((b.sections.len(), b.mois.len(), b.mois_caches), (14, MOIS_MAX, 12));
    }

    /// Ce qui manque à la saisie : un montant absent compte pour 0, et se compte — des deux côtés, et des dépenses
    /// seules pour l'état de la saisie.
    #[test]
    fn ce_qui_manque_a_la_saisie() {
        let f = |id, cote, montants: (Option<Cents>, Option<Cents>, Option<Cents>)| Facture {
            id,
            cote,
            statut: "ENVOYEE".into(),
            type_document: "FACTURE".into(),
            date: Some("2026-10-01".into()),
            sous_total: montants.0,
            tps: montants.1,
            tvq: montants.2,
            total: Some(100),
            ..Facture::default()
        };
        let r = Registre {
            factures: vec![
                f(1, Cote::Fournisseur, (None, Some(5), Some(10))),
                f(2, Cote::Fournisseur, (Some(85), Some(5), None)),
                f(3, Cote::Fournisseur, (Some(85), Some(5), Some(10))),
                f(4, Cote::Client, (Some(85), None, Some(10))),
                f(5, Cote::Client, (None, Some(5), Some(10))),
            ],
            ..Registre::default()
        };
        let b = calculer(&r, tout(), AUJOURDHUI);
        assert_eq!((b.depenses_sans_sous_total, b.depenses_sans_taxe), (1, 1));
        assert_eq!((b.sans_sous_total, b.sans_taxe), (2, 2));
        assert_eq!((b.depenses.sous_total, b.depenses.tvq, b.revenus.tps), (170, 20, 5));
    }

    #[test]
    fn la_moyenne_les_parts_et_la_fiabilite() {
        assert_eq!(
            (moyenne(1_001, 2), moyenne(-1_001, 2), moyenne(1_000, 3), moyenne(1_001, 3)),
            (501, -501, 333, 334)
        );
        assert_eq!(moyenne(5, 0), 0);
        assert_eq!(part(1, 2_000), Some("<\u{a0}0,1\u{a0}%".into()));
        assert_eq!(part(95, 1_000), Some("9,5\u{a0}%".into()));
        assert_eq!(part(995, 1_000), Some("100\u{a0}%".into()));
        assert_eq!((part(0, 10), part(5, 0), part(-5, 10)), (None, None, None));
        let avec = |v: Option<&str>| Facture { fiabilite: v.map(String::from), ..Facture::default() };
        assert_eq!(fiabilite(&avec(Some(" À vérifier "))), Fiabilite::AVerifier);
        assert_eq!(fiabilite(&avec(Some("SUR"))), Fiabilite::Sure);
        assert_eq!(fiabilite(&avec(Some("illisible"))), Fiabilite::NonJugee);
        assert_eq!(fiabilite(&avec(None)), Fiabilite::NonJugee);
    }
}
