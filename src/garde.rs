//! La garde : ce qui demande l'accord de la personne avant de partir, et ce qui ne part jamais.
//!
//! **Depuis le 2026-10-22, Eve a toute liberté** ([`evaluer`]) : rien ne se confirme plus, tout part, courriels
//! compris ; seuls les jetons et les clés restent refusés. La décision d'Eve ([`decider`]) n'y ajoute que ce qui ne se
//! défait pas, tant que la personne garde ses accords (le privilège `accords`, [`Espace::accords`]) : une suppression
//! demandée à la voix ([`suppression`]) et toute écriture dans Constructo AI ([`accord_erp`]) attendent « je confirme ».
//! Ce qui suit décrit l'examen détaillé ([`examiner`]), que garde l'essai d'Eve.
//!
//! Eve laisse l'Adjointe travailler en autonomie, comme `Adjointe.bat` (`--dangerously-skip-permissions`), mais
//! chaque outil passe d'abord ici : Eve branche un crochet `PreToolUse` à l'initialisation de Claude Code, qui lui
//! soumet chaque appel avant qu'il parte. Trois issues ([`Decision`]) :
//! - **libre** : l'outil part tel quel — lire, chercher, préparer un brouillon, classer un courriel ;
//! - **à confirmer** : Eve dit ce qui va s'écrire et attend « je confirme » ou un clic ;
//! - **refusé** : jamais, quoi qu'on dise — un courriel envoyé par script (la règle 1 du `CLAUDE.md`), le jeton
//!   d'Adjointe ou une clé lus.
//!
//! Les commandes (Bash, PowerShell, Monitor) passent par une **liste d'autorisation** depuis la revue de sécurité du
//! 2026-10-21 : une commande n'est libre que si chacun de ses morceaux est reconnu — une lecture, un script de
//! l'Adjointe qui simule (sans `--ecrire`), un brouillon, un rangement. Tout le reste se confirme : une redirection
//! vers un fichier, du code écrit sur le moment (`python -c`, un sous-shell), un programme inconnu, une cmdlet ou une
//! méthode qui modifie. La commande est d'abord lue comme le ferait le shell ([`lire`]) : ce qui est entre guillemets ou
//! apostrophes n'est que du texte, et les sous-commandes (`$(…)`, les blocs `{…}`, un `powershell -Command "…"`)
//! s'examinent comme des commandes à part entière. Dans le doute, on confirme : un accord de trop coûte une phrase, une
//! écriture de trop peut coûter cher. C'est une seconde barrière : les règles de l'Adjointe restent la première.

use std::sync::OnceLock;

use serde_json::Value;

use crate::parole;

/// L'issue d'un appel d'outil.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// L'outil part tel quel.
    Libre,
    /// L'outil attend l'accord de la personne : `quoi` se dit à voix haute, `detail` s'affiche.
    Confirmer { quoi: String, detail: String },
    /// L'outil ne part pas ; `raison` est rendue à l'Adjointe.
    Refuser { raison: String },
}

/// La longueur au-delà de laquelle un détail affiché est écourté.
const DETAIL_MAX: usize = 2000;
/// Au-delà de ce nombre de shells imbriqués, la garde confirme sans chercher plus loin.
const PROFONDEUR_MAX: u8 = 4;

const REGLE_1: &str = "Règle 1 : rien ne part par script. Le courriel reste au brouillon ; c'est la personne qui clique \
                       « Envoyer » dans Outlook.";
const SECRET: &str = "Les jetons (celui d'Adjointe, ceux de Constructo AI) et les clés ne se lisent pas, ne s'affichent \
                      pas et ne se recopient pas (CLAUDE.md) : les scripts et Eve les lisent eux-mêmes.";

/// Les scripts de l'Adjointe dont `--ecrire` ne fait que ranger une copie locale (courriels et pièces dans les
/// dossiers de chantier, lignes du journal) : la règle 7 les veut faits sans redemander.
const RANGEMENTS: [&str; 2] = ["classer_courriels.py", "journal.py"];

/// La règle 1 : ce qui enverrait un courriel par script (cherché dans la commande sans ses espaces).
const ENVOIS: [&str; 11] = [
    "--yes-send", ".send(", "['send']", "[\"send\"]", "invokemember(\"send\"", "invokemember('send'",
    "send-mailmessage", "sendmail", "smtplib", "smtpclient", "system.net.mail",
];

/// Ce qui viserait le jeton d'Adjointe (il ne sert qu'à la variable `EG_JETON` de la séance), ou les jetons de
/// Constructo AI (`connexion.bin` du dossier local : chiffrés pour la session de Windows, que Claude Code partage —
/// Eve seule les lit, et les lui remet elle-même). Le `.env` du dossier local de l'Adjointe, qui porte son jeton, est
/// reconnu à part (tout fichier `.env` ou `*.env`).
const SECRETS: [&str; 3] = ["eg_jeton", "--jeton-mcp", "connexion.bin"];

/// Les séparateurs de morceaux d'une commande, une fois lue.
const SEPARATEURS: [char; 9] = [';', '|', '&', '\n', '\r', '(', ')', '{', '}'];

/// Les commandes qui ne font que lire ou afficher : libres, quels que soient leurs arguments (les redirections, les
/// sous-commandes et les méthodes sont examinées à part). Bash, cmd et les alias de PowerShell.
const LECTURES: &[&str] = &[
    "ls", "dir", "cat", "type", "head", "tail", "more", "less", "grep", "egrep", "fgrep", "rg", "findstr", "wc",
    "sort", "uniq", "cut", "tr", "echo", "printf", "pwd", "cd", "chdir", "pushd", "popd", "which", "where",
    "where.exe", "whoami", "hostname", "date", "basename", "dirname", "realpath", "readlink", "stat", "file", "du",
    "df", "tasklist", "ipconfig", "ping", "nslookup", "systeminfo", "ver", "chcp", "cls", "clear", "sleep", "jq",
    "diff", "cmp", "md5sum", "sha1sum", "sha256sum", "tree", "nl", "column", "od", "xxd", "hexdump", "strings", "seq",
    "expr", "cygpath", "uname", "id", "[", "[[", "test", "gci", "gc", "gi", "gl", "gp", "gps", "gsv", "gm", "gcm",
    "sls", "select", "?", "measure", "group", "fl", "ft", "fw", "h", "history", "ghy", "gdr", "gwmi", "write", "sl",
    "pwd", "gu", "gv", "ogv",
];

/// Les verbes de PowerShell qui ne font que lire ou mettre en forme.
const VERBES_LECTURE: &[&str] = &[
    "get", "select", "where", "foreach", "sort", "group", "measure", "format", "test", "resolve", "join", "split",
    "compare", "convertto", "convertfrom", "write", "wait", "find", "search", "read",
];

/// Les verbes de PowerShell qui modifient quelque chose (cherchés partout dans la commande).
const VERBES_ECRITURE: &[&str] = &[
    "set", "new", "remove", "add", "clear", "copy", "move", "rename", "start", "stop", "restart", "invoke", "export",
    "out", "install", "uninstall", "register", "unregister", "enable", "disable", "update", "send", "expand",
    "compress", "unblock", "protect", "unprotect", "mount", "dismount", "suspend", "resume", "reset", "grant",
    "revoke", "block", "lock", "unlock", "publish", "unpublish", "save", "sync", "backup", "restore", "initialize",
    "repair", "tee", "enter", "connect", "disconnect", "import", "merge", "edit", "deny", "approve", "debug", "submit",
    "close",
];

/// Les cmdlets libres malgré leur verbe (`Invoke-RestMethod` et `Invoke-WebRequest` sont jugés à leurs paramètres,
/// en tête de morceau : voir [`http_powershell`]).
const CMDLETS_LIBRES: &[&str] = &[
    "invoke-restmethod", "invoke-webrequest", "out-string", "out-null", "out-host", "out-default", "out-gridview",
    "start-sleep", "import-csv", "import-clixml", "new-timespan", "new-guid", "set-location", "push-location",
    "pop-location", "clear-host", "add-member", "set-strictmode", "new-variable", "set-variable", "remove-variable",
    "clear-variable",
];

/// Les méthodes (d'un objet ou d'une classe .NET) qui écrivent, suppriment ou lancent quelque chose.
const METHODES: &[&str] = &[
    "delete(", "moveto(", "copyto(", "move(", "copy(", "saveas(", "save(", "writealltext(", "writeallbytes(",
    "writealllines(", "appendalltext(", "appendalllines(", "appendtext(", "create(", "createtext(", "createdirectory(",
    "createsubdirectory(", "openwrite(", "setvalue(", "execute(", "shellexecute(", "invoke(", "deletefile(",
    "deletefolder(", "deletedirectory(", "copyfile(", "movefile(", "copyhere(", "movehere(", "kill(", "start(",
    "encrypt(", "decrypt(", "setaccesscontrol(", "setattributes(", "getenvironmentvariables(",
    "setenvironmentvariable(",
];

/// Les méthodes qu'un `ForEach-Object Nom` (ou `% Nom`) appellerait sur chaque objet.
const METHODES_MEMBRE: &[&str] = &["delete", "moveto", "copyto", "kill", "encrypt", "decrypt", "create"];

/// Les mots-clés qui précèdent une commande : on regarde le mot suivant.
const MOTS_CLES_PASSER: &[&str] = &[
    "if", "then", "else", "elif", "do", "while", "until", "!", "time", "nohup", "command", "builtin", "exec", "elseif",
    "try",
];

/// Les mots-clés après lesquels le morceau n'est pas une commande (un en-tête de boucle, une déclaration).
const MOTS_CLES_FIN: &[&str] = &[
    "fi", "done", "esac", "for", "case", "in", "function", "param", "return", "break", "continue", "exit", "throw",
    "local", "declare", "typeset", "readonly", "unset", "shift", "begin", "process", "end", "catch", "finally",
    "switch", "default", "filter", "wait", "read", "true", "false", ":", "set", "export",
];

/// Ce que font quelques commandes connues, dit en mots (toute commande inconnue se confirme de toute façon).
const COMMANDES_CONNUES: &[(&str, &str)] = &[
    ("rm", "supprimer des fichiers"),
    ("del", "supprimer des fichiers"),
    ("erase", "supprimer des fichiers"),
    ("rd", "supprimer des dossiers"),
    ("rmdir", "supprimer des dossiers"),
    ("ri", "supprimer des fichiers"),
    ("unlink", "supprimer des fichiers"),
    ("mv", "déplacer ou renommer des fichiers"),
    ("move", "déplacer des fichiers"),
    ("mi", "déplacer des fichiers"),
    ("ren", "renommer des fichiers"),
    ("rename", "renommer des fichiers"),
    ("rni", "renommer des fichiers"),
    ("cp", "copier des fichiers"),
    ("copy", "copier des fichiers"),
    ("cpi", "copier des fichiers"),
    ("xcopy", "copier des fichiers"),
    ("robocopy", "copier des fichiers"),
    ("ni", "créer un fichier ou un dossier"),
    ("md", "créer un dossier"),
    ("mkdir", "créer un dossier"),
    ("touch", "créer un fichier"),
    ("sc", "écrire dans un fichier"),
    ("ac", "écrire dans un fichier"),
    ("clc", "vider un fichier"),
    ("tee", "écrire dans un fichier"),
    ("sp", "modifier le registre ou un fichier"),
    ("si", "modifier un élément"),
    ("iex", "exécuter du code"),
    ("icm", "exécuter du code"),
    ("eval", "exécuter du code"),
    ("source", "exécuter un script"),
    (".", "exécuter un script"),
    ("trap", "programmer une commande"),
    ("call", "exécuter un fichier de commandes"),
    ("saps", "lancer un programme"),
    ("start", "lancer un programme"),
    ("ii", "ouvrir un fichier avec son programme"),
    ("explorer", "ouvrir l'Explorateur"),
    ("explorer.exe", "ouvrir l'Explorateur"),
    ("notepad", "ouvrir un programme"),
    ("code", "ouvrir un programme"),
    ("kill", "arrêter un programme"),
    ("spps", "arrêter un programme"),
    ("taskkill", "arrêter un programme"),
    ("shutdown", "éteindre ou redémarrer le poste"),
    ("logoff", "fermer la session"),
    ("reg", "modifier le registre de Windows"),
    ("regedit", "modifier le registre de Windows"),
    ("setx", "changer une variable de Windows"),
    ("schtasks", "programmer une tâche"),
    ("sc.exe", "configurer un service de Windows"),
    ("net", "configurer le réseau ou les comptes"),
    ("netsh", "configurer le réseau"),
    ("wmic", "configurer le poste"),
    ("subst", "changer un lecteur"),
    ("icacls", "changer des permissions"),
    ("takeown", "changer des permissions"),
    ("attrib", "changer des attributs de fichiers"),
    ("chmod", "changer des permissions"),
    ("msiexec", "installer un logiciel"),
    ("winget", "installer un logiciel"),
    ("choco", "installer un logiciel"),
    ("pip", "installer un logiciel"),
    ("pip3", "installer un logiciel"),
    ("npm", "installer un logiciel"),
    ("npx", "lancer un programme"),
    ("cargo", "construire ou installer un logiciel"),
    ("wget", "télécharger un fichier"),
    ("awk", "exécuter du code (awk)"),
    ("perl", "exécuter du code"),
    ("ruby", "exécuter du code"),
    ("node", "exécuter du code"),
    ("php", "exécuter du code"),
    ("cscript", "exécuter du code"),
    ("wscript", "exécuter du code"),
    ("mshta", "exécuter du code"),
    ("rundll32", "exécuter du code"),
    ("certutil", "décoder ou télécharger un fichier"),
    ("ssh", "se connecter à un autre poste"),
    ("scp", "copier vers un autre poste"),
    ("tar", "extraire ou créer une archive"),
    ("unzip", "extraire une archive"),
    ("appel-programme", "lancer un programme"),
];

/// L'espace d'actions d'une Eve (le lot 1 d'`ETAT.md`) : ce que Claude peut y prendre, et ce qu'Eve y fait confirmer.
/// Claude Code ne reçoit que cela à son lancement (`--tools`, `--strict-mcp-config`, `--no-chrome`, les connecteurs
/// coupés) ; la garde refuse en plus tout outil qui en sort ([`Espace::refus`]), au cas où l'un passerait quand même.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Espace {
    /// Les connecteurs de claude.ai (Constructo AI, Gmail…) : des centaines d'outils, dont des paiements et des envois.
    pub connecteurs: bool,
    /// Chrome, avec les sessions ouvertes de la personne.
    pub chrome: bool,
    /// Les serveurs MCP du poste : ceux de l'utilisateur et du projet (`.mcp.json`) — FreeCAD, la base d'Entrepreneur
    /// Général.
    pub mcp_du_poste: bool,
    /// Constructo AI par la connexion d'Eve elle-même, au compte de la personne : le serveur [`SERVEUR_CONSTRUCTO`],
    /// qu'Eve branche dans Claude Code avec son jeton, et ses quelque 500 outils.
    pub constructo: bool,
    /// Les accords à la voix : avant ce qui ne se défait pas — une suppression demandée à la voix, toute écriture dans
    /// Constructo AI —, Eve demande « je confirme ». Sans eux, tout part, et le journal note ce qui les aurait demandés.
    /// Les jetons et les clés restent refusés dans les deux cas.
    pub accords: bool,
    /// Les outils de Claude Code permis (`--tools`) ; `None` : tous.
    pub outils: Option<Vec<String>>,
}

/// Les outils d'un spécialiste borné (la liberté refusée à l'installation) : lire, écrire et chercher dans les
/// fichiers, chercher sur le Web. Pas de shell.
pub const OUTILS_BORNES: [&str; 7] = ["Read", "Write", "Edit", "Glob", "Grep", "WebSearch", "WebFetch"];

/// Les outils de Claude Code qui ne servent qu'aux serveurs MCP (les trouver, lire leurs ressources) : permis avec eux.
const OUTILS_DU_MCP: [&str; 3] = ["ToolSearch", "ListMcpResourcesTool", "ReadMcpResourceTool"];

impl Espace {
    /// L'Adjointe d'Entrepreneur Général garde ce qu'elle a toujours eu : tous les outils, le serveur de la base et ceux
    /// du poste, les accords à la voix ; ni connecteur, ni Chrome, ni Constructo AI.
    pub fn adjointe() -> Espace {
        Espace { connecteurs: false, chrome: false, mcp_du_poste: true, constructo: false, accords: true, outils: None }
    }

    /// Un spécialiste, d'office, depuis le 24 octobre 2026 (« lui donner la liberté totale ») : tous les
    /// outils de Claude Code (l'outil Agent compris : travailler à plusieurs en parallèle), Chrome, les connecteurs de
    /// claude.ai, les serveurs MCP du poste, Constructo AI — la raison d'être du programme —, et les accords à la voix.
    pub fn specialiste() -> Espace {
        Espace { connecteurs: true, chrome: true, mcp_du_poste: true, constructo: true, accords: true, outils: None }
    }

    /// Un spécialiste borné : ses fichiers et le Web ; ni shell, ni connecteur, ni Chrome, ni serveur MCP du poste, ni
    /// Constructo AI. Ce que l'installateur écrit quand la personne refuse la liberté totale.
    pub fn borne() -> Espace {
        Espace {
            connecteurs: false,
            chrome: false,
            mcp_du_poste: false,
            constructo: false,
            accords: true,
            outils: Some(OUTILS_BORNES.iter().map(|o| o.to_string()).collect()),
        }
    }

    /// Un outil hors de l'espace : la raison du refus, rendue à Claude ; `None` s'il est permis. Un outil MCP se
    /// reconnaît à son serveur : `mcp__claude_ai_…` (un connecteur), `mcp__claude-in-chrome__…`,
    /// `mcp__constructo-ai__…` (Constructo AI, par la connexion d'Eve), les autres du poste.
    pub fn refus(&self, outil: &str) -> Option<String> {
        let permis = match outil.strip_prefix("mcp__") {
            Some(reste) => match reste.split("__").next().unwrap_or("") {
                s if s.starts_with("claude_ai_") => self.connecteurs,
                "claude-in-chrome" => self.chrome,
                SERVEUR_CONSTRUCTO => self.constructo,
                _ => self.mcp_du_poste,
            },
            None => match &self.outils {
                None => true,
                Some(liste) => {
                    liste.iter().any(|o| o.eq_ignore_ascii_case(outil))
                        || (OUTILS_DU_MCP.contains(&outil) && self.serveurs_mcp())
                }
            },
        };
        (!permis).then(|| {
            format!(
                "« {outil} » n'est pas permis à cette Eve : ses privilèges (reglages.json) ne le donnent pas. Dis à la \
                 personne ce qu'il faudrait ; c'est elle qui l'ajoute."
            )
        })
    }

    /// Un serveur MCP, au moins, peut entrer dans la séance.
    fn serveurs_mcp(&self) -> bool {
        self.connecteurs || self.chrome || self.mcp_du_poste || self.constructo
    }

    /// Des centaines d'outils MCP peuvent venir — les connecteurs de claude.ai, Constructo AI (486 outils le 22 octobre
    /// 2026) — : la recherche d'outils de Claude Code doit rester. Sans elle, il les charge tous à chaque tour (mesuré
    /// par l'édition Constructo d'Eve, sur 300 outils : 103 024 jetons pour un « OK », contre 10 043 avec elle).
    pub fn recherche_d_outils(&self) -> bool {
        self.connecteurs || self.constructo
    }

    /// La liste `--tools` de Claude Code ; `None` : tous ses outils. Une liste permise reçoit `ToolSearch` quand la
    /// recherche d'outils doit rester ([`Espace::recherche_d_outils`]) : une liste qui l'omet la coupe.
    pub fn outils_de_claude_code(&self) -> Option<Vec<String>> {
        let mut liste = self.outils.clone()?;
        if self.recherche_d_outils() && !liste.iter().any(|o| o.eq_ignore_ascii_case("ToolSearch")) {
            liste.push("ToolSearch".into());
        }
        Some(liste)
    }

    /// L'espace en une ligne, pour le journal, à chaque lancement de Claude Code.
    pub fn resume(&self) -> String {
        let outils = match &self.outils {
            None => "tous les outils".to_string(),
            Some(l) if l.is_empty() => "aucun outil".to_string(),
            Some(l) => format!("les outils {}", l.join(", ")),
        };
        let avec = |oui: bool, quoi: &str| format!("{}{quoi}", if oui { "avec " } else { "sans " });
        let accords = if self.accords {
            "avec les accords à la voix (« je confirme » avant une suppression demandée à la voix et toute écriture dans \
             Constructo AI)"
        } else {
            "sans accord : tout part sans « je confirme », et le journal note ce qui l'aurait demandé"
        };
        format!(
            "{outils} ; {} ; {} ; {} ; {} ; {accords} ; les jetons et les .env, jamais",
            avec(self.connecteurs, "les connecteurs de claude.ai"),
            avec(self.chrome, "Chrome"),
            avec(self.mcp_du_poste, "les serveurs MCP du poste"),
            avec(self.constructo, "Constructo AI")
        )
    }
}

// ---- Constructo AI -----------------------------------------------------------------------------------------------

/// Le serveur MCP de Constructo AI tel qu'Eve le branche elle-même dans Claude Code, avec sa propre connexion
/// (`connexion.rs`) : ses outils s'y appellent `mcp__constructo-ai__…`.
pub const SERVEUR_CONSTRUCTO: &str = "constructo-ai";

/// Les verbes de lecture des outils de Constructo AI, et les lectures que leur verbe ne dit pas : la copie exacte de
/// `mcp_profils.est_lecture` du serveur (`_VERBES_LECTURE`, `_OUTILS_LECTURE_EXPLICITES`), qui range ses outils pour les
/// profils d'entreprise — reprise telle quelle de l'édition Constructo d'Eve. C'est un nom, pas une preuve : l'essai
/// `les_outils_constructo_comme_le_serveur` compare la copie au serveur, outil par outil.
const VERBES_LECTURE_CONSTRUCTO: [&str; 11] = [
    "lister", "obtenir", "etat", "tableau", "statistiques", "comparer", "fiche", "decrire", "whoami", "rechercher",
    "previsions",
];
const LECTURES_CONSTRUCTO: &[&str] = &[
    "cao_quantites",
    "cao_resume_document",
    "cao_verifier_document",
    "cao_mesurer",
    "cao_chemins_noeuds",
    "cao_pi_po_vers_m",
    "cao_aide_script",
    "cao_emprise_escalier",
    "metre_resume_document",
    "metre_calculer_bordereau",
    "metre_simuler_formule",
    "metre_historique_modifications",
    "metre_echeancier_operations",
    "metre_echeancier_etat",
    "metre_aide_script",
    "config_lire_infos_employeur",
    "config_parametres",
    "bt_produits_manquants",
    "bt_codes_pointage",
    "bt_lister_produits",
    "comptabilite_aide_script",
    "calculer_beton",
    "calculer_toiture",
    "calculer_peinture",
    "calculer_electricite",
    "calculer_plomberie",
    "calculer_cvac",
    "calculer_escalier",
    "calculer_taxes_quebec",
    "conformite_expirations",
    "programmes_expirant_subventions",
    "hover_lister_jobs",
    "hover_obtenir_job",
];

/// Un outil de l'ERP de Constructo AI : celui de la connexion d'Eve (`mcp__constructo-ai__…`), ou celui du connecteur
/// de claude.ai (`mcp__claude_ai_Constructo_AI__…`, quand une Eve a les connecteurs : le nom du connecteur dit
/// « Constructo »). Rend son nom sans son serveur.
pub fn outil_erp(outil: &str) -> Option<&str> {
    let (serveur, nom) = outil.strip_prefix("mcp__")?.split_once("__")?;
    let connecteur = serveur.strip_prefix("claude_ai_").is_some_and(|c| c.to_ascii_lowercase().contains("constructo"));
    (serveur == SERVEUR_CONSTRUCTO || connecteur).then_some(nom).filter(|n| !n.is_empty())
}

/// Un outil de Constructo AI (son nom, sans son serveur) ne fait-il que lire ? Comme `mcp_profils._verbe_en_tete` : le
/// verbe est l'un des deux premiers segments du nom (`lister_factures`, `banque_lister_comptes`), jamais plus loin.
pub fn est_lecture_constructo(nom: &str) -> bool {
    LECTURES_CONSTRUCTO.contains(&nom) || nom.splitn(3, '_').take(2).any(|s| VERBES_LECTURE_CONSTRUCTO.contains(&s))
}

/// L'accord qu'une écriture dans Constructo AI demande, s'il en est une dans cet appel : une écriture dans l'ERP ne se
/// défait pas d'ici. Une lecture part ; la recherche SQL aussi, si elle ne fait que lire (le serveur n'admet que
/// SELECT : c'est une seconde barrière) ; un brouillon de courriel aussi — il ne part pas, la personne l'envoie de l'ERP
/// (le serveur n'expose aucun envoi de courriel). Tout le reste attend « je confirme » : créer, modifier, supprimer,
/// enregistrer un paiement, publier une opération bancaire, estimer un document de la maquette (qui crée un vrai devis),
/// lancer un script, envoyer un message, le SQL d'administration, et tout outil que le serveur ajoutera sans verbe de
/// lecture en tête. Ce qui se dit à voix haute nomme le geste et ce qu'il touche ; le détail affiché est l'appel entier.
pub fn accord_erp(outil: &str, entree: &Value) -> Option<Decision> {
    let nom = outil_erp(outil)?;
    match nom {
        "recherche_sql" => {
            let requete = champ(entree, "requete_sql");
            est_ecriture_sql(requete).then(|| confirmer("écrire dans Constructo AI, en SQL", requete))
        }
        "sql_admin" => Some(confirmer("lancer du SQL d'administration dans Constructo AI", &abreger(entree))),
        "email_creer_brouillon" => None,
        _ if est_lecture_constructo(nom) => None,
        _ => Some(confirmer(&action_erp(nom, entree), &format!("{nom}\n{}", abreger(entree)))),
    }
}

/// Les verbes des écritures de Constructo AI, tels qu'ils se disent ; les premiers prennent un article indéfini (« créer
/// une facture », « assigner un employé »), les autres le défini (« modifier la facture »).
const VERBES_ERP: &[(&str, &str, bool)] = &[
    ("creer", "créer", true),
    ("ajouter", "ajouter", true),
    ("enregistrer", "enregistrer", true),
    ("envoyer", "envoyer", true),
    ("importer", "importer", true),
    ("saisir", "saisir", true),
    ("declarer", "déclarer", true),
    ("generer", "générer", true),
    ("lancer", "lancer", true),
    ("assigner", "assigner", true),
    ("modifier", "modifier", false),
    ("supprimer", "supprimer", false),
    ("retirer", "retirer", false),
    ("soumettre", "soumettre", false),
    ("desassigner", "désassigner", false),
    ("changer", "changer", false),
    ("ajuster", "ajuster", false),
    ("definir", "définir", false),
    ("deplacer", "déplacer", false),
    ("publier", "publier", false),
    ("depublier", "dépublier", false),
    ("exclure", "exclure", false),
    ("exporter", "exporter", false),
    ("executer", "exécuter", false),
    ("annuler", "annuler", false),
    ("marquer", "marquer", false),
    ("etoiler", "étoiler", false),
    ("traiter", "traiter", false),
    ("pointer", "pointer", false),
    ("configurer", "configurer", false),
    ("reactiver", "réactiver", false),
    ("rattacher", "rattacher", false),
    ("detacher", "détacher", false),
    ("cloturer", "clôturer", false),
    ("engager", "engager", false),
    ("desengager", "désengager", false),
    ("lever", "lever", false),
    ("abandonner", "abandonner", false),
    ("dupliquer", "dupliquer", false),
    ("renommer", "renommer", false),
    ("copier", "copier", false),
    ("cloner", "cloner", false),
    ("appliquer", "appliquer", false),
    ("replanifier", "replanifier", false),
    ("reordonner", "réordonner", false),
    ("effacer", "effacer", false),
    ("valider", "valider", false),
    ("fermer", "fermer", false),
    ("estimer", "estimer", false),
    ("analyser", "analyser", false),
    ("grouper", "grouper", false),
    ("dissoudre", "dissoudre", false),
    ("aligner", "aligner", false),
    ("couper", "couper", false),
    ("percer", "percer", false),
    ("creuser", "creuser", false),
    ("evider", "évider", false),
    ("soustraire", "soustraire", false),
    ("unir", "unir", false),
    ("intersecter", "intersecter", false),
    ("incliner", "incliner", false),
    ("prolonger", "prolonger", false),
    ("imbriquer", "imbriquer", false),
    ("lier", "lier", false),
    ("calculer", "calculer", false),
];

/// Ce que touchent les écritures de Constructo AI (ce qui suit le verbe dans le nom de l'outil), avec l'article
/// indéfini et le défini. Les autres se disent par leurs mots.
const OBJETS_ERP: &[(&str, &str, &str)] = &[
    ("facture", "une facture", "la facture"),
    ("ligne_facture", "une ligne de facture", "la ligne de facture"),
    ("paiement", "un paiement", "le paiement"),
    ("devis", "un devis", "le devis"),
    ("projet", "un projet", "le projet"),
    ("note_projet", "une note au projet", "la note du projet"),
    ("entreprise", "une entreprise", "l'entreprise"),
    ("contact", "un contact", "le contact"),
    ("contact_principal", "un contact principal", "le contact principal"),
    ("fournisseur", "un fournisseur", "le fournisseur"),
    ("employe", "un employé", "l'employé"),
    // « assigner un employé au bon de travail », « désassigner un employé du bon de travail ».
    ("employe_bt", "un employé au bon de travail", "un employé du bon de travail"),
    ("produit", "un produit", "le produit"),
    ("produits", "des produits", "les produits"),
    ("stock", "du stock", "le stock"),
    ("bon_travail", "un bon de travail", "le bon de travail"),
    ("ligne_bt", "une ligne au bon de travail", "la ligne du bon de travail"),
    ("commentaire_bt", "un commentaire au bon de travail", "le commentaire du bon de travail"),
    ("statut_bt", "un statut de bon de travail", "le statut du bon de travail"),
    ("bon_achat", "un bon d'achat", "le bon d'achat"),
    ("bon_commande", "un bon de commande", "le bon de commande"),
    ("opportunite", "une opportunité", "l'opportunité"),
    ("interaction", "une interaction", "l'interaction"),
    ("pointage", "un pointage", "le pointage"),
    ("equipement", "un équipement", "l'équipement"),
    ("maintenance", "un entretien", "l'entretien"),
    ("maintenance_equipement", "un entretien d'équipement", "l'entretien de l'équipement"),
    ("reservation_equipement", "une réservation d'équipement", "la réservation d'équipement"),
    ("vehicule", "un véhicule", "le véhicule"),
    ("trajet_vehicule", "un trajet de véhicule", "le trajet du véhicule"),
    ("livraison", "une livraison", "la livraison"),
    ("article_livraison", "un article à la livraison", "l'article de la livraison"),
    ("dossier", "un dossier", "le dossier"),
    ("extra_dossier", "un extra au dossier", "l'extra du dossier"),
    ("attestation", "une attestation", "l'attestation"),
    ("carte_ccq", "une carte CCQ", "la carte CCQ"),
    ("licence_rbq", "une licence RBQ", "la licence RBQ"),
    ("contrat_location", "un contrat de location", "le contrat de location"),
    ("ligne_contrat_location", "une ligne au contrat de location", "la ligne du contrat de location"),
    ("article_location", "un article de location", "l'article de location"),
    ("retour_location", "un retour de location", "le retour de location"),
    ("contrat_employe_location", "un contrat d'employé en location", "le contrat de l'employé en location"),
    ("employe_location", "un employé en location", "l'employé en location"),
    ("heures_employe_location", "des heures d'un employé en location", "les heures de l'employé en location"),
    ("demande_subvention", "une demande de subvention", "la demande de subvention"),
    ("document_subvention", "un document de subvention", "le document de subvention"),
    ("statut_document_subvention", "le statut d'un document de subvention", "le statut du document de subvention"),
    ("ecriture_journal", "une écriture de journal", "l'écriture de journal"),
    ("coordination", "une coordination", "la coordination"),
    ("canal", "un canal de messagerie", "le canal de messagerie"),
    ("message", "un message", "le message"),
    ("alerte", "une alerte", "l'alerte"),
    ("alertes", "des alertes", "les alertes"),
    ("compte", "un compte bancaire", "le compte bancaire"),
    ("operation", "une opération", "l'opération"),
    ("lot", "un lot d'opérations", "le lot d'opérations"),
    ("releve", "un relevé bancaire", "le relevé bancaire"),
    ("demande", "une demande", "la demande"),
    ("intervention", "une intervention", "l'intervention"),
    ("planification", "une planification", "la planification"),
    ("compteur", "un relevé de compteur", "le compteur"),
    ("historique", "un historique", "l'historique"),
    ("piece", "une pièce", "la pièce"),
    ("type", "un type", "le type"),
    ("document", "un document", "le document"),
    ("dessin", "un dessin", "le dessin"),
    ("pdf", "un PDF", "le PDF"),
    ("dwg", "un fichier DWG", "le fichier DWG"),
    ("script", "un script", "le script"),
    ("commande", "une commande", "la commande"),
    ("traitement", "un traitement", "le traitement"),
    ("capture", "une capture", "la capture"),
    ("terrain", "un terrain", "le terrain"),
    ("lu", "comme lu", "comme lu"),
    ("mesure", "une mesure", "la mesure"),
    ("prix", "un prix", "le prix"),
    ("calque", "un calque", "le calque"),
    ("gabarit", "un gabarit", "le gabarit"),
    ("groupe", "un groupe", "le groupe"),
    ("formule", "une formule", "la formule"),
    ("modification", "une modification", "la modification"),
    ("derniere_modification", "la dernière modification", "la dernière modification"),
    ("carte", "une carte", "la carte"),
    ("note_calendrier", "une note au calendrier", "la note du calendrier"),
    ("evenement", "un événement", "l'événement"),
    ("contrainte", "une contrainte", "la contrainte"),
    ("semaine", "une semaine", "la semaine"),
    ("mur", "un mur", "le mur"),
    ("dalle", "une dalle", "la dalle"),
    ("etage", "un étage", "l'étage"),
    ("escalier", "un escalier", "l'escalier"),
    ("toit", "un toit", "le toit"),
    ("ouverture", "une ouverture", "l'ouverture"),
    ("materiau", "un matériau", "le matériau"),
];

/// Où se fait une écriture, d'après le module en tête du nom de l'outil (`cao_ajouter_mur`) ; sans module, l'ERP.
const LIEUX_ERP: &[(&str, &str)] = &[
    ("banque", "dans la banque de Constructo AI"),
    ("bt", "dans les bons de travail de Constructo AI"),
    ("cad", "dans un dessin de Constructo AI"),
    ("cao", "dans une maquette de Constructo AI"),
    ("comptabilite", "dans la comptabilité de Constructo AI"),
    ("email", "dans les courriels de Constructo AI"),
    ("maintenance", "dans la maintenance de Constructo AI"),
    ("metre", "dans un métré de Constructo AI"),
    ("photogrammetry", "dans la photogrammétrie de Constructo AI"),
    ("sdp", "dans la planification des chantiers de Constructo AI"),
    ("suivi", "dans le suivi des chantiers de Constructo AI"),
];

/// Ce que va faire une écriture dans Constructo AI, dit à voix haute après « J'ai besoin de ton accord pour » : son
/// verbe et ce qu'il touche, d'après le nom de l'outil — « créer une facture dans Constructo AI », « supprimer le projet
/// dans Constructo AI », « ajouter « mur » dans une maquette de Constructo AI » ; un paiement dit son montant. Un nom
/// sans verbe connu se dit tel quel.
fn action_erp(nom: &str, entree: &Value) -> String {
    let segments: Vec<&str> = nom.split('_').collect();
    let verbe = |s: &str| VERBES_ERP.iter().find(|(v, _, _)| *v == s);
    // Le verbe : le premier segment, ou le second après un module (`banque_`, `cao_`, `metre_`…), comme le serveur.
    let Some(i) = segments.iter().take(2).position(|s| verbe(s).is_some()) else {
        return format!("écrire dans Constructo AI ({})", segments.join(" "));
    };
    let (_, dit, indefini) = verbe(segments[i]).copied().unwrap_or(("", "écrire", true));
    let lieu = if i == 1 { LIEUX_ERP.iter().find(|(m, _)| *m == segments[0]).map(|(_, l)| *l) } else { None };
    let objet = segments[i + 1..].join("_");
    let objet = match OBJETS_ERP.iter().find(|(o, _, _)| *o == objet) {
        Some((_, un, le)) => format!(" {}", if indefini { un } else { le }),
        None if objet.is_empty() => String::new(),
        // « pointer par code », « couper par plan », « ajouter sur face » : une façon de faire, pas une chose.
        None if objet.starts_with("par_") || objet.starts_with("sur_") => format!(" {}", objet.replace('_', " ")),
        None => format!(" « {} »", objet.replace('_', " ")),
    };
    // Un paiement dit son montant : c'est lui qui décide (« 5214,90 dollars », sans espace qui le couperait à l'oral).
    let montant = match entree.get("montant").and_then(Value::as_f64) {
        Some(m) if nom.contains("paiement") && m.is_finite() && m > 0.0 => {
            let m = (m * 100.0).round() / 100.0;
            let texte = if m.fract() == 0.0 { format!("{m:.0}") } else { format!("{m:.2}").replace('.', ",") };
            format!(" de {texte} dollars")
        }
        _ => String::new(),
    };
    format!("{dit}{objet}{montant} {}", lieu.unwrap_or("dans Constructo AI"))
}

/// La décision d'Eve pour un outil : l'espace d'actions d'abord (un outil hors des privilèges ne part jamais), puis la
/// garde (les jetons et les clés, jamais), puis les accords ([`accord_voulu`]) — s'ils sont gardés ([`Espace::accords`]) :
/// sans eux, tout le reste part. `a_la_voix` : la demande du tour est venue de la voix.
pub fn decider(espace: &Espace, outil: &str, entree: &Value, a_la_voix: bool) -> Decision {
    if let Some(raison) = espace.refus(outil) {
        return Decision::Refuser { raison };
    }
    match evaluer(outil, entree) {
        Decision::Libre if espace.accords => accord_voulu(outil, entree, a_la_voix).unwrap_or(Decision::Libre),
        d => d,
    }
}

/// L'accord que demande ce qui ne se défait pas, que la personne garde ses accords ou non (sans eux, le journal le note
/// quand même) : toute écriture dans Constructo AI, la demande dite ou écrite — Eve ne la rattrape pas, et elle peut
/// venir de Claude seul ou d'un texte qu'il a lu ([`accord_erp`]) ; une suppression, quand la demande est venue de la
/// voix — une oreille peut se tromper de nom ([`suppression`]). Au clavier, le reste est libre.
pub fn accord_voulu(outil: &str, entree: &Value, a_la_voix: bool) -> Option<Decision> {
    accord_erp(outil, entree).or_else(|| {
        let detail = suppression(outil, entree).filter(|_| a_la_voix)?;
        Some(Decision::Confirmer { quoi: "supprimer".into(), detail })
    })
}

/// La garde d'Eve depuis le 2026-10-22 : **toute liberté**, comme Claude Code. Créer, modifier, supprimer, écrire dans
/// la base, lancer un programme, envoyer : tout part sans « je confirme ». Seuls les jetons et les clés restent hors de
/// portée (refus d'office) : ils ne se lisent ni ne se recopient, les scripts et Eve les lisent eux-mêmes. Les règles
/// du `CLAUDE.md` demeurent la seule barrière ; le chef y ajoute les accords ([`Espace::accords`]). L'examen détaillé
/// ([`examiner`]) reste pour l'essai d'Eve, qui refuse toute écriture.
pub fn evaluer(outil: &str, entree: &Value) -> Decision {
    match examiner(outil, entree) {
        Decision::Refuser { raison } if raison == SECRET => Decision::Refuser { raison },
        _ => Decision::Libre,
    }
}

/// Les mots d'une commande qui suppriment : le programme, la cmdlet, la méthode ou le geste d'un script. Comparés sans
/// leurs tirets de tête (`-delete` de `find`, `--delete` de `rsync` et de `git push`, `--supprimer`).
const MOTS_SUPPRESSION: &[&str] = &[
    "rm", "rmdir", "rd", "del", "erase", "unlink", "shred", "rmtree", "remove", "remove-item", "ri", "delete", "drop",
    "truncate", "supprimer", "effacer", "purger",
    // La corbeille que prescrit le CLAUDE.md, `[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile` ou
    // `DeleteDirectory` — qui, sur un lecteur `subst` comme M:, supprime pour de bon (mesuré) — et son jumeau Python,
    // `shellcon.FO_DELETE` ; vider la corbeille.
    "deletefile", "deletedirectory", "fo_delete", "clear-recyclebin",
];

/// Ce qui ne supprime qu'après son programme : `git clean`, `cargo clean`, `robocopy /MIR` ou `/PURGE` (qui effacent à
/// la destination ce que la source n'a pas). Seuls, « clean », « mir » et « purge » sont des mots du métier (« robinet
/// de purge ») ou des noms de clients (« Clean Énergie », « Mir Construction ») : une confirmation à chaque fois.
fn suppression_en_contexte(mots: &[&str]) -> bool {
    let a = |m: &str| mots.contains(&m);
    ((a("git") || a("cargo")) && a("clean")) || (a("robocopy") && (a("mir") || a("purge")))
}

/// Les mots d'un texte, en minuscules, sans leurs tirets de tête.
fn mots_de(texte: &str) -> Vec<String> {
    texte
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_'))
        .map(|m| m.trim_start_matches('-').to_string())
        .filter(|m| !m.is_empty())
        .collect()
}

/// Les changements d'un corps de `/api/changes` (`changes`, ou `lots[].changes`) qui retirent : leur type finit par
/// « - » (`chantier-`, qui efface aussi les heures, descriptions et sections du chantier ; `employe-` ; `section-`). Le
/// corps se lit en objet, ou en texte JSON.
fn retraits_du_corps(corps: Option<&Value>) -> Vec<String> {
    let lu;
    let corps = match corps {
        Some(Value::String(t)) => {
            lu = serde_json::from_str::<Value>(t).unwrap_or(Value::Null);
            &lu
        }
        Some(v) => v,
        None => return Vec::new(),
    };
    let mut listes: Vec<&Value> = corps.get("changes").into_iter().collect();
    if let Some(lots) = corps.get("lots").and_then(Value::as_array) {
        listes.extend(lots.iter().filter_map(|l| l.get("changes")));
    }
    listes
        .iter()
        .filter_map(|l| l.as_array())
        .flatten()
        .filter_map(|c| c.get("t").and_then(Value::as_str))
        .filter(|t| t.ends_with('-'))
        .map(str::to_string)
        .collect()
}

/// Une suppression, s'il en est une dans cet appel : ce qu'elle vise, pour le montrer. Eve la fait confirmer quand la
/// demande est venue de la voix — une oreille peut se tromper de nom, et une suppression ne se rattrape pas toujours.
/// Large exprès : une confirmation de trop coûte une phrase.
pub fn suppression(outil: &str, entree: &Value) -> Option<String> {
    let mots_qui_suppriment = |texte: &str| {
        let mots = mots_de(texte);
        let mots: Vec<&str> = mots.iter().map(String::as_str).collect();
        mots.iter().any(|m| MOTS_SUPPRESSION.contains(m)) || suppression_en_contexte(&mots)
    };
    match outil {
        "Bash" | "Monitor" | "PowerShell" => {
            let commande = champ(entree, "command");
            mots_qui_suppriment(commande).then(|| commande.to_string())
        }
        // Un sous-agent : ses propres outils pourraient ne pas repasser par le crochet d'Eve. Sa consigne parle-t-elle de
        // supprimer ?
        "Agent" | "Task" => {
            let consigne = format!("{} {}", champ(entree, "description"), champ(entree, "prompt"));
            let bas = parole::normaliser(&consigne);
            let racines = ["supprim", "effac", "detrui", "retire", "jette", "vide la corbeille", "delete", "remove"];
            (racines.iter().any(|r| bas.contains(r)) || mots_qui_suppriment(&consigne))
                .then(|| ecourter(&consigne, 600))
        }
        "mcp__entrepreneur-general__sql" => {
            let requete = champ(entree, "requete");
            let mots = mots_de(requete);
            let a = |m: &str| mots.iter().any(|x| x == m);
            // Le retrait doux que décrit la doc de la base : `UPDATE pl_plans SET retire = now()` (le fichier s'efface
            // 24 h après), `UPDATE qr_questionnaires SET supprime = now()` (la corbeille de 30 jours).
            let retrait_doux = a("update") && a("set") && (a("retire") || a("supprime"));
            (mots_qui_suppriment(requete) || retrait_doux).then(|| requete.to_string())
        }
        "mcp__entrepreneur-general__api" => {
            let methode = champ(entree, "methode").to_uppercase();
            let chemin = champ(entree, "chemin");
            let bas = chemin.to_lowercase();
            let retraits = retraits_du_corps(entree.get("corps"));
            (methode == "DELETE" || bas.contains("supprimer") || bas.contains("retirer") || !retraits.is_empty()).then(
                || {
                    let quoi = if retraits.is_empty() {
                        String::new()
                    } else {
                        format!(" — retire : {}", retraits.join(", "))
                    };
                    format!("{methode} {chemin}{quoi}")
                },
            )
        }
        _ if outil.starts_with("mcp__") => {
            let nom = outil.rsplit("__").next().unwrap_or(outil).to_lowercase();
            // Les noms anglais, et français : un serveur MCP d'ici dit « supprimer_facture », « retirer_article ».
            let racines = ["delete", "remove", "purge", "supprim", "retir", "effac"];
            racines.iter().any(|r| nom.contains(r)).then(|| format!("{outil}\n{}", abreger(entree)))
        }
        _ => None,
    }
}

/// Examine en détail un appel d'outil de Claude Code : son nom (`Bash`, `mcp__entrepreneur-general__sql`…) et son
/// entrée. Ce qui demanderait l'accord, ce qui serait refusé ; [`evaluer`] n'en garde que le refus du jeton.
pub fn examiner(outil: &str, entree: &Value) -> Decision {
    match outil {
        "Bash" | "Monitor" => commande(champ(entree, "command"), Shell::Bash),
        "PowerShell" => commande(champ(entree, "command"), Shell::Ps),
        "Read" | "Grep" => {
            let chemin = format!("{} {}", champ(entree, "file_path"), champ(entree, "path")).to_lowercase();
            if touche_un_secret(&chemin) { Decision::Refuser { raison: SECRET.into() } } else { Decision::Libre }
        }
        "Write" | "Edit" | "MultiEdit" | "NotebookEdit" => {
            let fichier = champ(entree, "file_path");
            let fichier = if fichier.is_empty() { champ(entree, "notebook_path") } else { fichier };
            if touche_un_secret(&fichier.to_lowercase()) {
                return Decision::Refuser { raison: SECRET.into() };
            }
            confirmer("modifier un fichier", fichier)
        }
        // Les outils d'un sous-agent pourraient ne pas repasser par le crochet de la garde.
        "Agent" | "Task" | "SendMessage" => confirmer("confier une tâche à un sous-agent", &abreger(entree)),
        "Artifact" => match champ(entree, "action") {
            "" | "publish" | "delete" | "pin" | "unpin" => {
                confirmer("publier ou retirer une page en ligne", &abreger(entree))
            }
            _ => Decision::Libre,
        },
        "ArtifactData" | "ArtifactComments" => match champ(entree, "action") {
            "get" | "list" | "query" | "read" => Decision::Libre,
            _ => confirmer("écrire dans une page en ligne", &abreger(entree)),
        },
        "SendUserFile" => confirmer("envoyer un fichier sur un autre appareil", &abreger(entree)),
        "RemoteTrigger" | "CronCreate" | "CronDelete" | "ScheduleWakeup" | "Workflow" | "EnterWorktree" => {
            confirmer("programmer ou lancer une tâche", &abreger(entree))
        }
        "mcp__entrepreneur-general__sql" => {
            let requete = champ(entree, "requete");
            if est_ecriture_sql(requete) { confirmer("écrire dans la base, en SQL", requete) } else { Decision::Libre }
        }
        "mcp__entrepreneur-general__api" => {
            let methode = champ(entree, "methode").to_uppercase();
            if methode.is_empty() || methode == "GET" {
                Decision::Libre
            } else {
                let chemin = champ(entree, "chemin");
                let corps = entree.get("corps").map(abreger).unwrap_or_default();
                confirmer("écrire dans la base, par son API", &format!("{methode} {chemin}\n{corps}"))
            }
        }
        // Avant la règle générale ci-dessous, dont les verbes sont anglais : `creer_facture`, `supprimer_projet` ou
        // `enregistrer_paiement` y passeraient sans accord.
        _ if outil_erp(outil).is_some() => accord_erp(outil, entree).unwrap_or(Decision::Libre),
        _ if outil.starts_with("mcp__") => {
            const OUTILS_MCP_ECRITURE: [&str; 26] = [
                "create", "update", "delete", "remove", "trigger", "deploy", "send", "post", "write", "upload",
                "publish", "set_", "put", "patch", "move", "rename", "execute", "run", "install", "batch",
                "form_input", "javascript", "computer", "navigate", "shortcuts_execute", "select_workspace",
            ];
            let nom = outil.rsplit("__").next().unwrap_or(outil).to_lowercase();
            if OUTILS_MCP_ECRITURE.iter().any(|m| nom.contains(m)) {
                confirmer("agir sur un service en ligne", &format!("{outil}\n{}", abreger(entree)))
            } else {
                Decision::Libre
            }
        }
        _ => Decision::Libre,
    }
}

/// Le shell d'une commande : sa façon de citer, d'échapper et d'enchaîner.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Shell {
    Bash,
    Ps,
    Cmd,
}

/// Le verdict d'un morceau de commande ; la commande entière devient le détail affiché.
enum Verdict {
    Libre,
    Confirmer(String),
    Refuser(String),
}

fn a_confirmer(quoi: &str) -> Verdict {
    Verdict::Confirmer(quoi.to_string())
}

/// Une commande Bash, PowerShell ou cmd.
fn commande(cmd: &str, sh: Shell) -> Decision {
    match analyser(cmd, sh, 0) {
        Verdict::Libre => Decision::Libre,
        Verdict::Confirmer(quoi) => confirmer(&quoi, cmd),
        Verdict::Refuser(raison) => Decision::Refuser { raison },
    }
}

fn analyser(cmd: &str, sh: Shell, profondeur: u8) -> Verdict {
    let bas = cmd.to_lowercase();
    let serre: String = bas.chars().filter(|c| !c.is_whitespace()).collect();
    if ENVOIS.iter().any(|m| serre.contains(m)) {
        return Verdict::Refuser(REGLE_1.into());
    }
    if touche_un_secret(&bas) {
        return Verdict::Refuser(SECRET.into());
    }
    if profondeur > PROFONDEUR_MAX {
        return a_confirmer("exécuter une commande trop imbriquée pour la garde");
    }
    if lit_l_environnement(&bas) {
        return a_confirmer("lire toutes les variables d'environnement (le jeton d'Adjointe y est)");
    }
    let lu = lire(cmd, sh);
    for s in &lu.sous {
        match analyser(s, sh, profondeur + 1) {
            Verdict::Libre => {}
            v => return v,
        }
    }
    let nu = lu.nu.to_lowercase();
    if redirection(&nu) {
        return a_confirmer("écrire dans un fichier (redirection)");
    }
    let nu_serre: String = nu.chars().filter(|c| !c.is_whitespace()).collect();
    if METHODES.iter().any(|m| nu_serre.contains(&format!(".{m}")) || nu_serre.contains(&format!("::{m}"))) {
        return a_confirmer("écrire, supprimer ou lancer quelque chose par une méthode .NET");
    }
    if nu_serre.contains("-comobject") {
        return a_confirmer("piloter un programme de Windows (objet COM)");
    }
    if let Some(q) = cmdlet_qui_ecrit(&nu) {
        return Verdict::Confirmer(q);
    }
    for seg in lu.texte.split(SEPARATEURS) {
        match segment(seg, sh, cmd, profondeur) {
            Verdict::Libre => {}
            v => return v,
        }
    }
    Verdict::Libre
}

/// La commande vise-t-elle le jeton d'Adjointe ou un fichier de clés (`.env`) ?
fn touche_un_secret(bas: &str) -> bool {
    SECRETS.iter().any(|s| bas.contains(s))
        || bas.split(|c: char| c.is_whitespace() || "\"'`;|&<>(){}=,".contains(c)).any(|m| {
            let m = m.trim_end_matches(['\\', '/']);
            let dernier = m.rsplit(['\\', '/']).next().unwrap_or(m);
            dernier == ".env" || (dernier.len() > 4 && dernier.ends_with(".env") && !dernier.contains('*'))
        })
}

/// La commande lit-elle toutes les variables d'environnement (`env`, `printenv`, `set` seuls, `dir env:`) ?
fn lit_l_environnement(bas: &str) -> bool {
    let seuls = bas
        .split(['\n', ';', '|', '&'])
        .map(str::trim)
        .any(|s| matches!(s, "env" | "printenv" | "set" | "export" | "export -p"));
    let fournisseur = bas
        .match_indices("env:")
        .any(|(p, _)| bas[p + 4..].chars().next().is_none_or(|c| c.is_whitespace() || "*\\/);|".contains(c)));
    seuls || fournisseur
}

/// Une commande lue comme le ferait le shell.
struct Lu {
    /// La commande, le texte des chaînes gardé mais rendu inerte (sans séparateur ni redirection).
    texte: String,
    /// La commande sans le texte de ses chaînes.
    nu: String,
    /// Les sous-commandes (`$(…)`, les accents graves de Bash, ce qu'un `@"…"@` ou un document en ligne développe).
    sous: Vec<String>,
}

/// Lit une commande : les apostrophes (et, en PowerShell, `@'…'@`) gardent du texte littéral ; entre guillemets, les
/// séparateurs et les redirections ne comptent pas, mais `$(…)` s'exécute ; un document en ligne de Bash (`<<FIN`)
/// n'est que du texte ; l'opérateur d'appel de PowerShell (`& 'programme.exe'`) devient un mot à part.
fn lire(cmd: &str, sh: Shell) -> Lu {
    let c: Vec<char> = cmd.chars().collect();
    let n = c.len();
    let mut lu = Lu { texte: String::with_capacity(n), nu: String::with_capacity(n), sous: Vec::new() };
    let pousser = |lu: &mut Lu, s: &str| {
        lu.texte.push_str(s);
        lu.nu.push_str(s);
    };
    // Un document en ligne de Bash, en attente de la fin de la ligne : son délimiteur, et s'il se développe.
    let mut document: Option<(String, bool, bool)> = None;
    let mut i = 0;
    while i < n {
        let ch = c[i];
        if ch == '\n'
            && let Some((fin, developpe, tirets)) = document.take()
        {
            let (apres, contenu) = corps_de_document(&c, i + 1, &fin, tirets);
            if developpe {
                lu.sous.extend(sous_commandes(&contenu, sh));
            }
            pousser(&mut lu, "\n");
            i = apres;
            continue;
        }
        if sh == Shell::Ps && ch == '@' && matches!(c.get(i + 1), Some('\'' | '"')) && fin_de_ligne(&c, i + 2) {
            let q = c[i + 1];
            let (apres, contenu) = chaine_multiligne(&c, i + 2, q);
            if q == '"' {
                lu.sous.extend(sous_commandes(&contenu, sh));
            }
            lu.texte.push_str(" \"\" ");
            lu.nu.push(' ');
            i = apres;
            continue;
        }
        if sh == Shell::Bash && ch == '<' && c.get(i + 1) == Some(&'<') && c.get(i + 2) != Some(&'<') {
            let (apres, fin, developpe, tirets) = delimiteur(&c, i + 2);
            document = Some((fin, developpe, tirets));
            pousser(&mut lu, " ");
            i = apres;
            continue;
        }
        // « 2>&1 » (un flux vers un autre) et « &> » (les deux flux vers un fichier) : des redirections, pas des
        // séparateurs ; le « & » y devient un caractère neutre (\u{1}).
        if ch == '>' && c.get(i + 1) == Some(&'&') {
            pousser(&mut lu, ">\u{1}");
            i += 2;
            continue;
        }
        if sh == Shell::Bash && ch == '&' && c.get(i + 1) == Some(&'>') {
            pousser(&mut lu, "\u{1}>");
            i += 2;
            continue;
        }
        match ch {
            '\'' if sh != Shell::Cmd => {
                let mut j = i + 1;
                while j < n {
                    if c[j] == '\'' {
                        if sh == Shell::Ps && c.get(j + 1) == Some(&'\'') {
                            j += 2;
                            continue;
                        }
                        break;
                    }
                    j += 1;
                }
                lu.texte.push('\'');
                lu.texte.extend(c[i + 1..j.min(n)].iter().map(|&x| inerte(x)));
                lu.texte.push('\'');
                lu.nu.push(' ');
                i = j + 1;
            }
            '"' => {
                let mut j = i + 1;
                lu.texte.push('"');
                while j < n {
                    let x = c[j];
                    if x == '"' {
                        if sh == Shell::Ps && c.get(j + 1) == Some(&'"') {
                            lu.texte.push(' ');
                            j += 2;
                            continue;
                        }
                        break;
                    }
                    if (sh == Shell::Bash && x == '\\') || (sh == Shell::Ps && x == '`') {
                        lu.texte.push(' ');
                        j += 2;
                        continue;
                    }
                    if sh != Shell::Cmd && x == '$' && c.get(j + 1) == Some(&'(') {
                        let (dedans, apres) = parenthese(&c, j + 1);
                        lu.sous.push(dedans);
                        lu.texte.push(' ');
                        j = apres;
                        continue;
                    }
                    if sh == Shell::Bash && x == '`' {
                        let fin = c[j + 1..].iter().position(|&y| y == '`').map(|p| j + 1 + p).unwrap_or(n);
                        lu.sous.push(c[(j + 1).min(n)..fin].iter().collect());
                        lu.texte.push(' ');
                        j = fin + 1;
                        continue;
                    }
                    lu.texte.push(inerte(x));
                    j += 1;
                }
                lu.texte.push('"');
                lu.nu.push(' ');
                i = j + 1;
            }
            // Un caractère échappé hors des chaînes : il vaut pour lui-même.
            '\\' if sh == Shell::Bash => {
                if let Some(&x) = c.get(i + 1) {
                    pousser(&mut lu, &inerte(x).to_string());
                }
                i += 2;
            }
            '`' if sh == Shell::Ps => {
                if let Some(&x) = c.get(i + 1) {
                    pousser(&mut lu, &inerte(x).to_string());
                }
                i += 2;
            }
            '^' if sh == Shell::Cmd => {
                if let Some(&x) = c.get(i + 1) {
                    pousser(&mut lu, &inerte(x).to_string());
                }
                i += 2;
            }
            '`' if sh == Shell::Bash => {
                let fin = c[i + 1..].iter().position(|&y| y == '`').map(|p| i + 1 + p).unwrap_or(n);
                lu.sous.push(c[(i + 1).min(n)..fin].iter().collect());
                pousser(&mut lu, " x ");
                i = fin + 1;
            }
            '$' if sh != Shell::Cmd && c.get(i + 1) == Some(&'(') => {
                let (dedans, apres) = parenthese(&c, i + 1);
                // $(( … )) : de l'arithmétique, pas une commande.
                if c.get(i + 2) != Some(&'(') {
                    lu.sous.push(dedans);
                }
                pousser(&mut lu, " x ");
                i = apres;
            }
            '<' | '>' if sh == Shell::Bash && c.get(i + 1) == Some(&'(') => {
                let (dedans, apres) = parenthese(&c, i + 1);
                lu.sous.push(dedans);
                pousser(&mut lu, " x ");
                i = apres;
            }
            '&' if sh == Shell::Ps => {
                if c.get(i + 1) == Some(&'&') {
                    pousser(&mut lu, " ; ");
                    i += 2;
                } else {
                    pousser(&mut lu, " ; appel-programme ");
                    i += 1;
                }
            }
            _ => {
                lu.texte.push(ch);
                lu.nu.push(ch);
                i += 1;
            }
        }
    }
    lu
}

/// Un caractère de chaîne, rendu inerte : ni séparateur, ni redirection, ni bloc.
fn inerte(x: char) -> char {
    if ";|&<>(){}\n\r`".contains(x) { ' ' } else { x }
}

/// Le texte entre une parenthèse ouvrante (à `ouvre`) et sa fermante, et l'index qui suit celle-ci.
fn parenthese(c: &[char], ouvre: usize) -> (String, usize) {
    let mut profondeur = 0usize;
    let mut guillemet: Option<char> = None;
    let mut j = ouvre;
    while j < c.len() {
        let x = c[j];
        match guillemet {
            Some(q) if x == q => guillemet = None,
            Some(_) => {}
            None => match x {
                '\'' | '"' => guillemet = Some(x),
                '(' => profondeur += 1,
                ')' => {
                    profondeur = profondeur.saturating_sub(1);
                    if profondeur == 0 {
                        return (c[ouvre + 1..j].iter().collect(), j + 1);
                    }
                }
                _ => {}
            },
        }
        j += 1;
    }
    (c[(ouvre + 1).min(c.len())..].iter().collect(), c.len())
}

/// Les sous-commandes d'un texte qui se développe (`$(…)`, et les accents graves de Bash).
fn sous_commandes(texte: &str, sh: Shell) -> Vec<String> {
    let c: Vec<char> = texte.chars().collect();
    let mut sous = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '$' && c.get(i + 1) == Some(&'(') {
            let (dedans, apres) = parenthese(&c, i + 1);
            sous.push(dedans);
            i = apres;
        } else if sh == Shell::Bash && c[i] == '`' {
            let fin = c[i + 1..].iter().position(|&y| y == '`').map(|p| i + 1 + p).unwrap_or(c.len());
            sous.push(c[i + 1..fin].iter().collect());
            i = fin + 1;
        } else {
            i += 1;
        }
    }
    sous
}

/// Le caractère à `k` commence-t-il une fin de ligne (`\n` ou `\r\n`) ?
fn fin_de_ligne(c: &[char], k: usize) -> bool {
    c.get(k) == Some(&'\n') || (c.get(k) == Some(&'\r') && c.get(k + 1) == Some(&'\n'))
}

/// Une chaîne multiligne de PowerShell (`@'` ou `@"` déjà lus) : l'index qui suit sa fermeture (`'@` ou `"@` en début
/// de ligne), et son contenu.
fn chaine_multiligne(c: &[char], debut: usize, q: char) -> (usize, String) {
    let mut j = debut;
    while j < c.len() {
        if c[j] == q && c.get(j + 1) == Some(&'@') && j > 0 && c[j - 1] == '\n' {
            return (j + 2, c[debut..j].iter().collect());
        }
        j += 1;
    }
    (c.len(), c[debut.min(c.len())..].iter().collect())
}

/// Le délimiteur d'un document en ligne de Bash (`<<` déjà lu) : l'index qui le suit, le délimiteur, s'il se développe
/// (sans guillemets) et s'il tolère des tabulations (`<<-`).
fn delimiteur(c: &[char], mut j: usize) -> (usize, String, bool, bool) {
    let tirets = c.get(j) == Some(&'-');
    if tirets {
        j += 1;
    }
    while c.get(j).is_some_and(|x| *x == ' ' || *x == '\t') {
        j += 1;
    }
    let mut fin = String::new();
    let mut cite = false;
    while let Some(&x) = c.get(j) {
        if x.is_whitespace() || ";|&<>()".contains(x) {
            break;
        }
        if x == '\'' || x == '"' || x == '\\' {
            cite = true;
        } else {
            fin.push(x);
        }
        j += 1;
    }
    (j, fin, !cite, tirets)
}

/// Le corps d'un document en ligne, de `debut` à la ligne qui ne porte que le délimiteur : l'index qui suit cette
/// ligne, et le corps.
fn corps_de_document(c: &[char], debut: usize, fin: &str, tirets: bool) -> (usize, String) {
    let mut corps = String::new();
    let mut j = debut;
    while j < c.len() {
        let k = c[j..].iter().position(|&x| x == '\n').map(|p| j + p).unwrap_or(c.len());
        let ligne: String = c[j..k].iter().collect();
        let l = ligne.trim_end_matches('\r');
        let l = if tirets { l.trim_start_matches('\t') } else { l };
        if l == fin {
            return (k, corps);
        }
        corps.push_str(&ligne);
        corps.push('\n');
        j = k + 1;
    }
    (c.len(), corps)
}

/// Une redirection vers un fichier (`>`, `>>`) : `2>&1` et `> $null` (ou `/dev/null`, `nul`) ne comptent pas.
fn redirection(nu: &str) -> bool {
    let c: Vec<char> = nu.chars().collect();
    let mut i = 0;
    while i < c.len() {
        if c[i] != '>' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if c.get(j) == Some(&'>') {
            j += 1;
        }
        while c.get(j).is_some_and(|x| *x == ' ' || *x == '\t') {
            j += 1;
        }
        if matches!(c.get(j), Some('&' | '\u{1}')) {
            i = j + 1;
            continue;
        }
        let cible: String = c[j..].iter().take_while(|x| !x.is_whitespace() && !";|&)".contains(**x)).collect();
        if !["$null", "/dev/null", "nul", "null", "nul:", "/dev/stderr", "/dev/stdout"].contains(&cible.as_str()) {
            return true;
        }
        i = j;
    }
    false
}

/// Une cmdlet qui modifie quelque chose, où qu'elle soit dans la commande (dans un bloc `{ … }` par exemple).
fn cmdlet_qui_ecrit(nu: &str) -> Option<String> {
    nu.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).find_map(|m| {
        let (verbe, nom) = m.split_once('-')?;
        if nom.is_empty() || nom.contains('-') || CMDLETS_LIBRES.contains(&m) {
            return None;
        }
        (VERBES_ECRITURE.contains(&verbe) || m == "format-volume").then(|| quoi_cmdlet(m).to_string())
    })
}

/// Ce que fait une cmdlet, d'après son verbe.
fn quoi_cmdlet(nom: &str) -> &'static str {
    if nom.ends_with("-computer") {
        return "éteindre ou redémarrer le poste";
    }
    match nom.split('-').next().unwrap_or("") {
        "remove" | "clear" => "supprimer ou vider quelque chose",
        "start" | "invoke" | "enter" | "connect" | "import" | "debug" => "lancer un programme ou du code",
        "stop" | "restart" | "suspend" | "resume" => "arrêter ou redémarrer un programme",
        "install" | "uninstall" | "register" | "unregister" | "enable" | "disable" | "update" | "initialize"
        | "repair" | "reset" | "mount" | "dismount" | "format" => "installer ou configurer le poste",
        "send" | "publish" | "unpublish" | "submit" => "envoyer ou publier quelque chose",
        "grant" | "revoke" | "block" | "lock" | "unlock" | "protect" | "unprotect" | "deny" | "approve" => {
            "changer des permissions"
        }
        _ => "écrire ou modifier des fichiers ou des réglages",
    }
}

/// Les mots d'un morceau de commande ; les guillemets regroupent (et sont retirés).
fn mots(seg: &str) -> Vec<String> {
    let mut mots = Vec::new();
    let mut courant = String::new();
    let mut guillemet: Option<char> = None;
    let mut a_un_mot = false;
    for ch in seg.chars() {
        match guillemet {
            Some(q) if ch == q => guillemet = None,
            Some(_) => courant.push(ch),
            None if ch == '"' || ch == '\'' => {
                guillemet = Some(ch);
                a_un_mot = true;
            }
            None if ch.is_whitespace() => {
                if a_un_mot || !courant.is_empty() {
                    mots.push(std::mem::take(&mut courant));
                    a_un_mot = false;
                }
            }
            None => courant.push(ch),
        }
    }
    if a_un_mot || !courant.is_empty() {
        mots.push(courant);
    }
    mots
}

/// Un morceau de commande : libre seulement s'il est reconnu.
fn segment(seg: &str, sh: Shell, brut: &str, profondeur: u8) -> Verdict {
    let debut = seg.trim_start();
    // PowerShell : une affectation (« $x = Get-ChildItem … », une entrée de table « n='Taille' ») vaut sa valeur ; une
    // expression ne lance rien.
    if sh == Shell::Ps {
        let premier_mot = debut.split_whitespace().next().unwrap_or("");
        if debut.starts_with(['$', '[']) || (premier_mot.contains('=') && !premier_mot.starts_with('-')) {
            return match egal_hors_guillemets(debut) {
                Some(p) => segment(debut[p + 1..].trim_start_matches('='), sh, brut, profondeur),
                None => Verdict::Libre,
            };
        }
    }
    let m = mots(seg);
    let mut i = 0;
    loop {
        let Some(mot) = m.get(i) else { return Verdict::Libre };
        let b = mot.to_lowercase();
        // Bash : VAR=valeur commande.
        if sh == Shell::Bash && est_affectation(&b) {
            i += 1;
            continue;
        }
        if MOTS_CLES_PASSER.contains(&b.as_str()) {
            i += 1;
            continue;
        }
        if MOTS_CLES_FIN.contains(&b.as_str()) {
            return Verdict::Libre;
        }
        // Les lanceurs : timeout, nice, env, xargs — la commande qu'ils lancent suit leurs options.
        if matches!(b.as_str(), "timeout" | "nice" | "env" | "xargs") {
            i += 1;
            while m.get(i).is_some_and(|x| {
                x.starts_with(['-', '/']) || x.parse::<f64>().is_ok() || est_affectation(&x.to_lowercase()) || x == "{}"
            }) {
                i += 1;
            }
            continue;
        }
        break;
    }
    let premier = m[i].to_lowercase();
    let reste: Vec<String> = m[i + 1..].to_vec();
    let a = |x: &str| reste.iter().any(|r| r.eq_ignore_ascii_case(x));
    // Un programme désigné par son chemin, ou cité : Bash et cmd lancent un mot cité ; PowerShell l'affiche (il faut
    // l'opérateur « & » pour le lancer).
    let cite = m.len() > i && seg.trim_start().starts_with(['"', '\'']) && i == 0;
    let par_chemin = match sh {
        Shell::Ps => !cite && premier.contains(['/', '\\']),
        _ => cite || premier.contains(['/', '\\']),
    };
    match premier.as_str() {
        "python" | "python3" | "py" | "python.exe" | "python3.exe" => python(&reste),
        p if p.ends_with(".py") => python(&m[i..]),
        "git" | "git.exe" => git(&reste),
        "gh" => gh(&reste),
        "claude" | "claude.exe" => {
            if reste.iter().all(|r| ["--version", "-v", "--help", "-h"].contains(&r.to_lowercase().as_str())) {
                Verdict::Libre
            } else {
                a_confirmer("changer la configuration de Claude Code")
            }
        }
        "curl" | "curl.exe" => curl(&reste),
        "invoke-restmethod" | "irm" | "invoke-webrequest" | "iwr" => http_powershell(&reste),
        "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe" => imbrique(brut, &premier, Shell::Ps, profondeur),
        "cmd" | "cmd.exe" => imbrique(brut, &premier, Shell::Cmd, profondeur),
        "bash" | "bash.exe" | "sh" | "wsl" | "wsl.exe" => imbrique(brut, &premier, Shell::Bash, profondeur),
        "find" | "find.exe" if sh != Shell::Cmd => {
            let ecrit = ["-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprintf", "-fls"];
            if reste.iter().any(|r| ecrit.contains(&r.to_lowercase().as_str())) {
                a_confirmer("supprimer ou modifier des fichiers (find)")
            } else {
                Verdict::Libre
            }
        }
        "sed" => {
            if reste.iter().any(|r| r.starts_with("-i") || r == "--in-place") {
                a_confirmer("modifier un fichier (sed)")
            } else {
                Verdict::Libre
            }
        }
        "fsutil" => {
            let r: Vec<String> = reste.iter().map(|x| x.to_lowercase()).collect();
            if r.first().map(String::as_str) == Some("fsinfo")
                || (r.first().map(String::as_str) == Some("file")
                    && r.get(1).map(String::as_str) == Some("queryvaliddata"))
            {
                Verdict::Libre
            } else {
                a_confirmer("changer un réglage de disque (fsutil)")
            }
        }
        "printenv" if reste.is_empty() => {
            a_confirmer("lire toutes les variables d'environnement (le jeton d'Adjointe y est)")
        }
        "printenv" => Verdict::Libre,
        "foreach-object" | "foreach" | "%" => {
            if reste.iter().any(|r| METHODES_MEMBRE.contains(&r.to_lowercase().as_str())) || a("-membername") {
                a_confirmer("appeler une méthode qui modifie sur chaque élément")
            } else {
                Verdict::Libre
            }
        }
        // PowerShell : une chaîne seule s'affiche.
        _ if sh == Shell::Ps && cite => Verdict::Libre,
        _ if par_chemin => a_confirmer("lancer un programme"),
        p if LECTURES.contains(&p) => Verdict::Libre,
        p if let Some((_, quoi)) = COMMANDES_CONNUES.iter().find(|(c, _)| *c == p) => a_confirmer(quoi),
        p if p.contains('-') && p.split('-').count() == 2 && !p.starts_with('-') => cmdlet(p),
        // PowerShell : une valeur seule (un tableau, un nombre, une négation, la suite d'une liste) s'affiche, comme le
        // membre d'une expression entre parenthèses (« (Get-Item x).Length »).
        p if sh == Shell::Ps && p.starts_with(['@', '-', '!', '+', ',']) => Verdict::Libre,
        p if sh == Shell::Ps && p.starts_with('.') && p.chars().nth(1).is_some_and(char::is_alphabetic) => {
            Verdict::Libre
        }
        p if sh == Shell::Ps && p.chars().next().is_some_and(|c| c.is_ascii_digit()) => Verdict::Libre,
        p => Verdict::Confirmer(format!("lancer une commande que la garde ne connaît pas (« {p} »)")),
    }
}

/// La position du premier « = » hors des guillemets et des apostrophes.
fn egal_hors_guillemets(s: &str) -> Option<usize> {
    let mut guillemet: Option<char> = None;
    for (p, ch) in s.char_indices() {
        match guillemet {
            Some(q) if ch == q => guillemet = None,
            Some(_) => {}
            None if ch == '"' || ch == '\'' => guillemet = Some(ch),
            None if ch == '=' => return Some(p),
            None => {}
        }
    }
    None
}

fn est_affectation(b: &str) -> bool {
    match b.split_once('=') {
        Some((nom, _)) => {
            !nom.is_empty()
                && nom.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && nom.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// Une cmdlet en tête de morceau.
fn cmdlet(nom: &str) -> Verdict {
    let verbe = nom.split('-').next().unwrap_or("");
    if CMDLETS_LIBRES.contains(&nom) || (VERBES_LECTURE.contains(&verbe) && nom != "format-volume") {
        Verdict::Libre
    } else if VERBES_ECRITURE.contains(&verbe) || nom == "format-volume" {
        a_confirmer(quoi_cmdlet(nom))
    } else {
        Verdict::Confirmer(format!("lancer une commande que la garde ne connaît pas (« {nom} »)"))
    }
}

/// Le dossier de l'Adjointe, en minuscules, avec des « / » et sans « / » final : ses `scripts/` se reconnaissent aussi
/// par leur chemin absolu. Posé une fois, au démarrage ([`reconnaitre_dossier`]).
static DOSSIER_ADJOINTE: OnceLock<Vec<String>> = OnceLock::new();

/// Fait reconnaître à la garde le dossier de l'Adjointe : `python "<dossier>/scripts/x.py"` y vaut
/// `python scripts/x.py`. Sous les deux écritures d'un chemin Windows (`C:/…`, et `/c/…` de Git Bash). Une seule fois
/// par programme : un second appel ne change rien.
pub fn reconnaitre_dossier(dossier: &str) {
    let d = dossier.to_lowercase().replace('\\', "/");
    let d = d.trim_end_matches('/').to_string();
    if d.is_empty() {
        return;
    }
    let mut formes = vec![d.clone()];
    let mut c = d.chars();
    if let (Some(lecteur), Some(':')) = (c.next(), c.next())
        && lecteur.is_ascii_alphabetic()
    {
        formes.push(format!("/{lecteur}{}", &d[2..]));
    }
    let _ = DOSSIER_ADJOINTE.set(formes);
}

/// Le nom d'un script de l'Adjointe donné par son chemin absolu (`<dossier>/scripts/x.py`), s'il en est un.
fn script_du_dossier(s: &str) -> Option<String> {
    DOSSIER_ADJOINTE
        .get()?
        .iter()
        .find_map(|d| s.strip_prefix(d.as_str()).and_then(|r| r.strip_prefix("/scripts/")))
        .map(str::to_string)
}

/// Python : libre pour un script de l'Adjointe qui simule ; le code écrit sur le moment se confirme.
fn python(args: &[String]) -> Verdict {
    let mut i = 0;
    while let Some(a) = args.get(i) {
        let b = a.to_lowercase();
        match b.as_str() {
            "-c" => return a_confirmer("exécuter du code Python écrit sur le moment"),
            "-" => return a_confirmer("exécuter du code Python lu sur l'entrée"),
            "-m" => {
                let module = args.get(i + 1).map(|m| m.to_lowercase()).unwrap_or_default();
                return if matches!(module.as_str(), "json.tool" | "py_compile") {
                    Verdict::Libre
                } else {
                    Verdict::Confirmer(format!("lancer le module Python « {module} »"))
                };
            }
            "--version" | "-v" | "--help" | "-h" if args.len() == i + 1 => return Verdict::Libre,
            "-x" | "-w" => i += 2,
            _ if b.starts_with('-') => i += 1,
            _ => break,
        }
    }
    let Some(script) = args.get(i) else { return a_confirmer("exécuter du code Python lu sur l'entrée") };
    let s = script.to_lowercase().replace('\\', "/");
    let s = s.strip_prefix("./").unwrap_or(&s);
    let Some(nom) = s.strip_prefix("scripts/").map(str::to_string).or_else(|| script_du_dossier(s)) else {
        return a_confirmer("lancer un programme Python hors des scripts de l'Adjointe");
    };
    if nom.contains('/') || !nom.ends_with(".py") {
        return a_confirmer("lancer un programme Python hors des scripts de l'Adjointe");
    }
    let reste: Vec<String> = args[i + 1..].iter().map(|a| a.to_lowercase()).collect();
    let a = |m: &str| reste.iter().any(|x| x == m);
    if a("--ecrire") {
        return if RANGEMENTS.contains(&nom.as_str()) { Verdict::Libre } else { a_confirmer(quoi_ecrire(&nom)) };
    }
    match nom.as_str() {
        "outlook_mail.py" => {
            // La sous-commande, après les options générales (--account ADRESSE, --json).
            let mut k = 0;
            while let Some(x) = reste.get(k) {
                match x.as_str() {
                    "--account" => k += 2,
                    _ if x.starts_with("--") => k += 1,
                    _ => break,
                }
            }
            match reste.get(k).map(String::as_str) {
                Some("send") => Verdict::Refuser(REGLE_1.into()),
                Some("delete" | "move") => a_confirmer("déplacer ou supprimer un courriel"),
                _ => Verdict::Libre,
            }
        }
        "nouveau_projet.py" if a("creer") || a("template") => a_confirmer("créer un dossier de projet"),
        "quitter.py" if a("ranger") || a("--fermer") => {
            a_confirmer("ranger la journée (et éteindre le poste avec --fermer)")
        }
        "trieur_courriels.py" if a("go") || a("reel") || a("mode") => {
            a_confirmer("changer le mode du trieur des courriels")
        }
        "installer_jeton_base.py" if !a("--verifier") => a_confirmer("toucher au jeton d'Adjointe"),
        _ => Verdict::Libre,
    }
}

/// Ce que fait un `--ecrire`, dit en mots, d'après le script qu'il vise.
fn quoi_ecrire(script: &str) -> &'static str {
    const SCRIPTS: [(&str, &str); 8] = [
        ("compta_base.py", "inscrire une facture reçue dans la base"),
        ("revenus_base.py", "inscrire une facture client dans la base"),
        ("demandes_base.py", "écrire une demande de prix dans la base"),
        ("fournisseurs_base.py", "écrire une fiche fournisseur dans la base"),
        ("soumission_base.py", "écrire une soumission dans la base"),
        ("calendrier_en_ligne.py", "écrire dans le calendrier en ligne"),
        ("fichiers_vivants_compta.py", "refaire les fichiers vivants comptables"),
        ("nouveau_projet.py", "créer ou compléter un dossier de projet"),
    ];
    SCRIPTS.iter().find(|(s, _)| *s == script).map(|(_, q)| *q).unwrap_or("écrire (dans la base ou les dossiers)")
}

/// git : libre pour lire un dépôt, à confirmer pour le modifier.
fn git(reste: &[String]) -> Verdict {
    let r: Vec<String> = reste.iter().map(|x| x.to_lowercase()).collect();
    let mut i = 0;
    while let Some(x) = r.get(i) {
        match x.as_str() {
            "-c" => i += 2,
            _ if x.starts_with('-') => i += 1,
            _ => break,
        }
    }
    let sous = r.get(i).map(String::as_str).unwrap_or("");
    let apres = r.get(i + 1..).unwrap_or(&[]);
    let options_seules = apres.iter().all(|x| x.starts_with('-'));
    let destructrices = [
        "-d", "-D", "-m", "-M", "-c", "-C", "--delete", "--move", "--copy", "-f", "--force", "-u", "--set-upstream-to",
    ];
    let libre = match sous {
        "status" | "log" | "diff" | "show" | "rev-parse" | "ls-files" | "ls-tree" | "blame" | "describe"
        | "shortlog" | "grep" | "cat-file" | "whatchanged" | "help" | "version" | "" => true,
        "branch" => options_seules && !apres.iter().any(|x| destructrices.contains(&x.as_str())),
        "remote" => {
            apres.is_empty() || apres == ["-v"] || matches!(apres.first().map(String::as_str), Some("show" | "get-url"))
        }
        "config" => {
            apres.iter().any(|x| matches!(x.as_str(), "--get" | "--list" | "-l" | "--get-all" | "--get-regexp"))
        }
        "tag" => apres.iter().all(|x| x == "-l" || x == "--list" || x.starts_with("-n")),
        "stash" | "worktree" | "reflog" => matches!(apres.first().map(String::as_str), None | Some("list" | "show")),
        _ => false,
    };
    if libre { Verdict::Libre } else { a_confirmer("modifier un dépôt de code (git)") }
}

/// gh : libre pour lire GitHub.
fn gh(reste: &[String]) -> Verdict {
    let r: Vec<String> = reste.iter().map(|x| x.to_lowercase()).collect();
    let lit = match (r.first().map(String::as_str), r.get(1).map(String::as_str)) {
        (
            Some("pr" | "issue" | "repo" | "run" | "release" | "workflow"),
            Some("view" | "list" | "status" | "diff" | "checks"),
        ) => true,
        (Some("api"), _) => !r.iter().any(|x| {
            matches!(x.as_str(), "-x" | "--method" | "-f" | "--field" | "--raw-field" | "--input")
                || x.starts_with("--method=")
        }),
        (Some("auth"), Some("status")) => true,
        _ => false,
    };
    if lit { Verdict::Libre } else { a_confirmer("agir sur GitHub") }
}

/// curl : libre pour une lecture (GET) affichée ; un envoi de données ou un fichier écrit se confirme.
fn curl(reste: &[String]) -> Verdict {
    let pas_get = |v: &str| !matches!(v.trim_matches(['"', '\'']).to_uppercase().as_str(), "GET" | "HEAD");
    for (k, a) in reste.iter().enumerate() {
        let ecrit = if let Some(long) = a.strip_prefix("--") {
            let (nom, valeur) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            match nom {
                "request" => pas_get(&valeur.or_else(|| reste.get(k + 1).cloned()).unwrap_or_default()),
                n => {
                    n.starts_with("data")
                        || n.starts_with("form")
                        || n.starts_with("remote-name")
                        || n.starts_with("trace")
                        || matches!(
                            n,
                            "json" | "upload-file" | "output" | "output-dir" | "config" | "cookie-jar" | "dump-header"
                        )
                }
            }
        } else if a.len() > 1 && a.starts_with('-') {
            // Des options courtes, groupées (-sSL) ; -X prend une valeur, collée ou à part.
            let lettres: Vec<char> = a[1..].chars().collect();
            let mut e = false;
            for (p, l) in lettres.iter().enumerate() {
                match l {
                    'd' | 'F' | 'T' | 'o' | 'O' | 'K' | 'c' | 'D' => {
                        e = true;
                        break;
                    }
                    'X' => {
                        let collee: String = lettres[p + 1..].iter().collect();
                        let v = if collee.is_empty() { reste.get(k + 1).cloned().unwrap_or_default() } else { collee };
                        e = pas_get(&v);
                        break;
                    }
                    _ => {}
                }
            }
            e
        } else {
            false
        };
        if ecrit {
            return a_confirmer("envoyer des données en ligne ou écrire un fichier (curl)");
        }
    }
    Verdict::Libre
}

/// Invoke-RestMethod, Invoke-WebRequest : libres pour une lecture (GET) ; un corps, un fichier ou une autre méthode se
/// confirment. PowerShell accepte un début de nom de paramètre (`-Me Post`, `-B $corps`).
fn http_powershell(reste: &[String]) -> Verdict {
    for (k, a) in reste.iter().enumerate() {
        let b = a.to_lowercase();
        let Some(nom) = b.strip_prefix('-') else { continue };
        let (nom, collee) = match nom.split_once(':') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (nom, None),
        };
        if nom.is_empty() {
            continue;
        }
        let debut_de = |complet: &str| complet.starts_with(nom);
        if debut_de("body") || debut_de("infile") || debut_de("outfile") || debut_de("form") {
            return a_confirmer("envoyer des données en ligne ou écrire un fichier");
        }
        if nom.len() >= 2 && debut_de("method") {
            let v = collee.or_else(|| reste.get(k + 1).map(|x| x.to_lowercase())).unwrap_or_default();
            if !matches!(v.trim_matches(['"', '\'']), "get" | "head") {
                return a_confirmer("envoyer des données en ligne");
            }
        }
    }
    Verdict::Libre
}

/// Un shell lancé par la commande (`powershell -Command "…"`, `cmd /c …`, `bash -c '…'`) : la commande qu'il reçoit
/// s'examine à son tour, avec ses propres règles de citation ; un script, du code encodé ou un shell ouvert sans
/// commande se confirment.
fn imbrique(brut: &str, nom: &str, dedans: Shell, profondeur: u8) -> Verdict {
    let ascii = brut.to_ascii_lowercase();
    let Some(p) = ascii.match_indices(nom).map(|(p, _)| p).find(|&p| {
        let avant = ascii[..p].chars().next_back();
        avant.is_none_or(|c| c.is_whitespace() || "\"'&;|(/\\".contains(c))
    }) else {
        return a_confirmer("ouvrir un autre shell");
    };
    let apres = &brut[p + nom.len()..];
    let mut curseur = 0;
    let mut suite: Option<&str> = None;
    for mot in apres.split_whitespace() {
        let debut_mot = curseur + apres[curseur..].find(mot).unwrap_or(0);
        curseur = debut_mot + mot.len();
        let b = mot.to_ascii_lowercase();
        let b = b.trim_matches(['"', '\'']);
        match dedans {
            Shell::Ps => {
                if let Some(n) = b.strip_prefix('-') {
                    if !n.is_empty() && "command".starts_with(n) {
                        suite = Some(&apres[curseur..]);
                        break;
                    }
                    if n.starts_with('e') && "encodedcommand".starts_with(n) {
                        return a_confirmer("exécuter du code encodé (PowerShell)");
                    }
                    if n.starts_with('f') && "file".starts_with(n) {
                        return a_confirmer("exécuter un script PowerShell");
                    }
                    continue;
                }
                if [
                    "bypass", "hidden", "unrestricted", "remotesigned", "normal", "minimized", "maximized", "text",
                    "xml",
                ]
                .contains(&b)
                {
                    continue;
                }
                // Un argument nu vaut -Command.
                suite = Some(&apres[debut_mot..]);
                break;
            }
            Shell::Cmd => {
                if b == "/c" || b == "/k" {
                    suite = Some(&apres[curseur..]);
                    break;
                }
                if !b.starts_with('/') {
                    return a_confirmer("ouvrir une invite de commandes");
                }
            }
            Shell::Bash => {
                if b == "-c" {
                    suite = Some(&apres[curseur..]);
                    break;
                }
                if !b.starts_with('-') {
                    return a_confirmer("exécuter un script shell");
                }
            }
        }
    }
    let Some(suite) = suite else { return a_confirmer("ouvrir un autre shell sans commande") };
    let s = suite.trim();
    let s = match (s.chars().next(), s.chars().last()) {
        (Some(a), Some(z)) if s.len() >= 2 && a == z && (a == '"' || a == '\'') => &s[1..s.len() - 1],
        (Some(a), _) if a == '"' || a == '\'' => &s[1..],
        _ => s,
    };
    analyser(s, dedans, profondeur + 1)
}

/// Une requête SQL modifie-t-elle la base ? Les commentaires et les chaînes sont retirés d'abord, pour qu'un « update »
/// écrit dans un texte ne compte pas ; un `SELECT … INTO table` crée une table.
pub fn est_ecriture_sql(requete: &str) -> bool {
    const MOTS_SQL_ECRITURE: [&str; 23] = [
        "insert", "update", "delete", "merge", "upsert", "alter", "drop", "truncate", "create", "grant", "revoke",
        "comment", "vacuum", "reindex", "cluster", "copy", "call", "do", "lock", "refresh", "reassign", "security",
        "into",
    ];
    let nettoyee = sans_commentaires_ni_chaines(requete).to_lowercase();
    nettoyee.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|m| MOTS_SQL_ECRITURE.contains(&m))
}

/// Retire d'une requête SQL ses commentaires (`--`, `/* */`) et ses chaînes (`'…'`, `$$…$$`).
fn sans_commentaires_ni_chaines(sql: &str) -> String {
    let c: Vec<char> = sql.chars().collect();
    let mut sortie = String::with_capacity(sql.len());
    let mut i = 0;
    while i < c.len() {
        if c[i] == '-' && c.get(i + 1) == Some(&'-') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if c[i] == '/' && c.get(i + 1) == Some(&'*') {
            i += 2;
            while i < c.len() && !(c[i] == '*' && c.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i += 2;
            sortie.push(' ');
        } else if c[i] == '\'' {
            i += 1;
            while i < c.len() {
                if c[i] == '\'' && c.get(i + 1) == Some(&'\'') {
                    i += 2;
                } else if c[i] == '\'' {
                    break;
                } else {
                    i += 1;
                }
            }
            i += 1;
            sortie.push(' ');
        } else if c[i] == '$' && c.get(i + 1) == Some(&'$') {
            // Un corps entre $$ : on garde le mot qui précède (DO, CREATE FUNCTION), on retire le corps.
            i += 2;
            while i < c.len() && !(c[i] == '$' && c.get(i + 1) == Some(&'$')) {
                i += 1;
            }
            i += 2;
            sortie.push(' ');
        } else {
            sortie.push(c[i]);
            i += 1;
        }
    }
    sortie
}

/// Un accord à demander ; le détail affiché est écourté, et le dit.
fn confirmer(quoi: &str, detail: &str) -> Decision {
    let n = detail.chars().count();
    let detail = if n > DETAIL_MAX {
        format!("⚠ Commande écourtée à l'écran : elle compte {n} caractères en tout.\n{}", ecourter(detail, DETAIL_MAX))
    } else {
        detail.to_string()
    };
    Decision::Confirmer { quoi: quoi.to_string(), detail }
}

/// Un champ texte de l'entrée d'un outil, vide s'il manque.
fn champ<'a>(entree: &'a Value, nom: &str) -> &'a str {
    entree.get(nom).and_then(Value::as_str).unwrap_or("")
}

/// Une entrée JSON, en texte court.
fn abreger(v: &Value) -> String {
    ecourter(&v.to_string(), DETAIL_MAX)
}

/// Écourte un texte à `max` caractères (jamais au milieu d'un caractère).
pub fn ecourter(texte: &str, max: usize) -> String {
    if texte.chars().count() <= max {
        return texte.to_string();
    }
    let mut s: String = texte.chars().take(max).collect();
    s.push_str(" […]");
    s
}

#[cfg(test)]
mod essais {
    #[test]
    fn une_suppression_se_reconnait() {
        use serde_json::json;
        for (outil, entree) in [
            ("Bash", json!({"command": "rm -rf \"/m/Dossier Clients/024. Client\""})),
            ("PowerShell", json!({"command": "Remove-Item -Recurse 'C:/temp/x'"})),
            ("Bash", json!({"command": "python -c \"import shutil; shutil.rmtree('x')\""})),
            ("Bash", json!({"command": "python scripts/projets.py supprimer 24"})),
            ("mcp__entrepreneur-general__sql", json!({"requete": "delete from projets where id = 1"})),
            ("mcp__entrepreneur-general__api", json!({"methode": "DELETE", "chemin": "/api/employes/3"})),
            (
                "mcp__entrepreneur-general__api",
                json!({"methode": "POST", "chemin": "/qr/api/questionnaires/7/supprimer"}),
            ),
            ("Agent", json!({"description": "Ménage", "prompt": "Supprime les doublons du dossier Scan"})),
            ("Task", json!({"description": "Nettoyage", "prompt": "Efface les vieux brouillons"})),
            ("Bash", json!({"command": "git clean -fdx"})),
            ("Bash", json!({"command": "cargo clean"})),
            ("PowerShell", json!({"command": "robocopy \"C:/Projets\" \"D:/Copie\" /MIR"})),
            ("PowerShell", json!({"command": "robocopy C:/a C:/b /E /PURGE"})),
            // La corbeille du CLAUDE.md, et son jumeau Python : sur M: (un `subst`), une suppression définitive.
            (
                "PowerShell",
                json!({"command": "[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($p,'OnlyErrorDialogs','SendToRecycleBin')"}),
            ),
            (
                "PowerShell",
                json!({"command": "[Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($d,'OnlyErrorDialogs','SendToRecycleBin')"}),
            ),
            ("Bash", json!({"command": "python -c \"shell.SHFileOperation((0, shellcon.FO_DELETE, p, None, 0))\""})),
            ("PowerShell", json!({"command": "Clear-RecycleBin -Force"})),
            ("Bash", json!({"command": "find \"/m/Scan\" -name '*.pdf' -mtime +30 -delete"})),
            ("Bash", json!({"command": "rsync -a --delete src/ dst/"})),
            ("Bash", json!({"command": "python scripts/projets.py --supprimer 24"})),
            // Le corps de /api/changes : un type qui finit par « - » retire (objet, lots, ou texte JSON).
            (
                "mcp__entrepreneur-general__api",
                json!({"methode": "POST", "chemin": "/api/changes", "corps": {"changes": [{"t": "chantier-", "id": "c012trem"}]}}),
            ),
            (
                "mcp__entrepreneur-general__api",
                json!({"methode": "POST", "chemin": "/api/changes",
                       "corps": {"id": "x", "base": 3, "lots": [{"lot": "abcdefgh", "changes": [{"t": "employe-", "nom": "Paul"}]}]}}),
            ),
            (
                "mcp__entrepreneur-general__api",
                json!({"methode": "POST", "chemin": "/api/changes", "corps": "{\"changes\":[{\"t\":\"section-\",\"id\":\"s1\"}]}"}),
            ),
            // Le retrait doux, en SQL.
            (
                "mcp__entrepreneur-general__sql",
                json!({"requete": "UPDATE pl_plans SET retire = now(), retire_ip = 'Adjointe' WHERE id = 7"}),
            ),
            (
                "mcp__entrepreneur-general__sql",
                json!({"requete": "update qr_questionnaires set supprime = now() where id = 3"}),
            ),
            ("mcp__constructo-ai__supprimer_facture", json!({"facture_id": 3})),
            ("mcp__constructo-ai__retirer_article_livraison", json!({"article_id": 9})),
            ("mcp__autre__effacer_note", json!({"id": 2})),
            ("mcp__autre__purger_cache", json!({})),
        ] {
            assert!(suppression(outil, &entree).is_some(), "{outil} {entree}");
        }
        for (outil, entree) in [
            ("Bash", json!({"command": "python scripts/nouveau_projet.py creer \"Client\""})),
            ("Bash", json!({"command": "ls \"/m/Dossier Clients\""})),
            ("Bash", json!({"command": "git status"})),
            ("PowerShell", json!({"command": "robocopy C:/a C:/b /E"})),
            // « purge », « mir », « clean » sans leur programme : des mots du métier, des noms de clients.
            ("Bash", json!({"command": "python scripts/journal.py seance \"Purge du chauffe-eau chez Tremblay\""})),
            (
                "Bash",
                json!({"command": "python scripts/outlook_mail.py --json draft --to a@b.ca --subject \"Valve de purge\" --body \"x\""}),
            ),
            ("Bash", json!({"command": "ls \"/m/Dossier Clients/032. Mir Construction\""})),
            ("Bash", json!({"command": "ls \"/m/Dossier Clients/031. Clean Énergie Inc\""})),
            (
                "mcp__entrepreneur-general__api",
                json!({"methode": "POST", "chemin": "/api/changes", "corps": {"changes": [{"t": "chantier", "id": "c012", "nom": "Garage"}]}}),
            ),
            ("mcp__entrepreneur-general__sql", json!({"requete": "select id, nom from pl_plans where retire is null"})),
            ("mcp__constructo-ai__lister_factures", json!({})),
            ("mcp__constructo-ai__creer_facture", json!({"client_id": 4})),
            ("mcp__entrepreneur-general__sql", json!({"requete": "update projets set nom = 'x' where id = 1"})),
            ("mcp__entrepreneur-general__api", json!({"methode": "POST", "chemin": "/qr/api/questionnaires"})),
            ("Write", json!({"file_path": "C:/temp/x.md"})),
            ("Agent", json!({"description": "Recherche", "prompt": "Trouve les factures de Canac de 2025"})),
        ] {
            assert!(suppression(outil, &entree).is_none(), "{outil} {entree}");
        }
    }

    #[test]
    fn eve_a_toute_liberte_sauf_le_jeton() {
        use serde_json::json;
        for (outil, entree) in [
            ("Write", json!({"file_path": "M:/Dossier Clients/024. Client/notes.md"})),
            ("Bash", json!({"command": "rm -rf \"/m/Dossier Clients/024. Client/Archives\""})),
            ("Bash", json!({"command": "python scripts/nouveau_projet.py creer \"Client\""})),
            ("PowerShell", json!({"command": "Remove-Item C:/temp/x.txt"})),
            ("mcp__entrepreneur-general__sql", json!({"requete": "DELETE FROM projets WHERE id = 1"})),
            ("mcp__entrepreneur-general__api", json!({"methode": "POST", "chemin": "/qr/api/questionnaires"})),
            ("Bash", json!({"command": "python scripts/outlook.py envoyer --yes-send"})),
        ] {
            assert_eq!(evaluer(outil, &entree), Decision::Libre, "{outil} {entree}");
        }
        assert!(matches!(evaluer("Bash", &json!({"command": "echo $EG_JETON"})), Decision::Refuser { .. }));
        assert!(matches!(
            evaluer("Bash", &json!({"command": "python scripts/adjointe_config.py --jeton-mcp"})),
            Decision::Refuser { .. }
        ));
    }

    use super::*;
    use serde_json::json;

    fn bash(cmd: &str) -> Decision {
        examiner("Bash", &json!({ "command": cmd }))
    }

    fn ps(cmd: &str) -> Decision {
        examiner("PowerShell", &json!({ "command": cmd }))
    }

    fn confirme(d: &Decision) -> bool {
        matches!(d, Decision::Confirmer { .. })
    }

    fn refuse(d: &Decision) -> bool {
        matches!(d, Decision::Refuser { .. })
    }

    #[test]
    fn lire_et_chercher_sont_libres() {
        assert_eq!(bash("python scripts/outlook_mail.py list --folder inbox --limit 25"), Decision::Libre);
        assert_eq!(bash("python -u scripts/matin.py"), Decision::Libre);
        assert_eq!(bash("python -u scripts/matin.py --poste"), Decision::Libre);
        assert_eq!(bash("python scripts/compta_base.py chercher --numero 15182"), Decision::Libre);
        assert_eq!(bash("cd \"C:/x y\" && python scripts/compta_base.py chercher --numero 1"), Decision::Libre);
        assert_eq!(bash("python scripts/x.py 2>&1 | head -n 20"), Decision::Libre);
        assert_eq!(bash("ls -la; cat fichier | grep -i facture | wc -l"), Decision::Libre);
        assert_eq!(bash("echo $(date)"), Decision::Libre);
        assert_eq!(bash("PYTHONIOENCODING=utf-8 python -I scripts/journal.py voir --date 2026-10-20"), Decision::Libre);
        // Le dossier de l'Adjointe, par son chemin absolu : sous ses deux écritures, et seulement lui.
        reconnaitre_dossier(r"C:\Users\x\Mon Entreprise\Eve");
        assert_eq!(bash("python \"C:/Users/x/Mon Entreprise/Eve/scripts/matin.py\" --poste"), Decision::Libre);
        assert_eq!(bash("python \"/c/Users/x/Mon Entreprise/Eve/scripts/matin.py\" --poste"), Decision::Libre);
        assert!(confirme(&bash("python \"C:/Users/x/Autre/scripts/matin.py\" --poste")));
        assert_eq!(bash("if [ -f x ]; then cat x; fi"), Decision::Libre);
        assert_eq!(bash("git status && git log --oneline -5"), Decision::Libre);
        assert_eq!(bash("curl -s https://entrepreneur-general.ca/api/meteo"), Decision::Libre);
        assert_eq!(bash("curl -fsSL -H \"Accept: application/json\" https://x.ca/a"), Decision::Libre);
        assert_eq!(
            bash("powershell -NoProfile -Command \"Get-Process Outlook -ErrorAction SilentlyContinue\""),
            Decision::Libre
        );
        assert_eq!(examiner("Read", &json!({ "file_path": "C:/x.md" })), Decision::Libre);
        assert_eq!(examiner("Grep", &json!({ "pattern": "delete" })), Decision::Libre);
    }

    #[test]
    fn powershell_qui_lit_est_libre() {
        assert_eq!(ps("Get-ChildItem \"C:\\x\" | Select-Object Name, Length"), Decision::Libre);
        assert_eq!(ps("Get-Content x.md -TotalCount 20"), Decision::Libre);
        assert_eq!(
            ps("Get-ChildItem | Where-Object { $_.Length -gt 1MB } | Sort-Object Length -Descending"),
            Decision::Libre
        );
        assert_eq!(ps("Get-ChildItem | Select-Object @{n='Taille';e={$_.Length}}, Name"), Decision::Libre);
        assert_eq!(ps("$f = Get-Item 'C:\\x'; $f.Length"), Decision::Libre);
        assert_eq!(ps("Get-Process | ForEach-Object { \"$($_.Name) : $($_.Id)\" }"), Decision::Libre);
        assert_eq!(ps("Test-Path $env:LOCALAPPDATA\\EntrepreneurGeneral"), Decision::Libre);
        assert_eq!(ps("[System.IO.File]::ReadAllText('C:\\x.md').Length"), Decision::Libre);
        assert_eq!(ps("Get-Date -Format 'yyyy-MM-dd HH:mm'"), Decision::Libre);
        assert_eq!(ps("Invoke-RestMethod https://entrepreneur-general.ca/api/calendrier"), Decision::Libre);
        assert_eq!(ps("(Get-ChildItem C:\\x -Recurse).Count"), Decision::Libre);
        assert_eq!(ps("(Get-Content x.md) -join ' '"), Decision::Libre);
        assert_eq!(ps("python scripts/x.py 2>&1 | Select-Object -First 5"), Decision::Libre);
        assert_eq!(ps("if (Test-Path x) { Get-Content x } else { 'absent' }"), Decision::Libre);
    }

    #[test]
    fn un_brouillon_est_libre_un_envoi_jamais() {
        assert_eq!(
            bash(
                "python scripts/outlook_mail.py draft --to a@b.ca --subject \"Facture 1234; relance\" --body \"Bonjour Martin & Julie > merci\""
            ),
            Decision::Libre
        );
        assert_eq!(
            bash("python scripts/outlook_mail.py --account \"martin@exemple.ca\" reply --id ABC --body \"Merci\""),
            Decision::Libre
        );
        assert!(refuse(&bash("python scripts/outlook_mail.py send --id ABC --yes-send")));
        assert!(refuse(&bash("python scripts/outlook_mail.py send --id ABC")));
        assert!(refuse(&ps("$m.Send()")));
        assert!(refuse(&ps("$m.Send( )")));
        assert!(refuse(&ps("$m.psobject.Methods['Send'].Invoke()")));
        assert!(refuse(&ps("Send-MailMessage -To a@b.ca -Subject x")));
    }

    #[test]
    fn ecrire_dans_la_base_se_confirme() {
        let d = bash("python scripts/compta_base.py inscrire --numero 1 --ecrire");
        assert_eq!(
            d,
            Decision::Confirmer {
                quoi: "inscrire une facture reçue dans la base".into(),
                detail: "python scripts/compta_base.py inscrire --numero 1 --ecrire".into()
            }
        );
        assert!(confirme(&bash("python scripts/calendrier_en_ligne.py ecrire F.json --ecrire")));
    }

    #[test]
    fn ranger_un_courriel_ou_le_journal_est_libre() {
        assert_eq!(bash("python scripts/classer_courriels.py pdf-classer --projet 020 --ecrire"), Decision::Libre);
        assert_eq!(bash("python scripts/journal.py courriels --ecrire"), Decision::Libre);
        assert_eq!(bash("python scripts/classer_courriels.py ecrire --confirmer 393fe2ac=005"), Decision::Libre);
        assert_eq!(bash("python scripts/journal.py suspens --fait \"rm du gypse\""), Decision::Libre);
    }

    /// Les constats de la revue de sécurité du 2026-10-21 (H1 à H4, M1 à M4, B1).
    #[test]
    fn les_contournements_de_la_revue_se_confirment() {
        // H1 : un nom de script de rangement ailleurs dans la commande ne lève plus l'accord.
        assert!(confirme(&bash("python scripts/compta_base.py inscrire --ecrire ; echo journal.py")));
        // H2 : écrire un fichier par le shell.
        assert!(confirme(&ps("Set-Content scripts\\compta_base.py \"x\"")));
        assert!(confirme(&bash("echo malveillant > scripts/compta_base.py")));
        assert!(confirme(&bash("echo x >> CLAUDE.md")));
        assert!(confirme(&ps("Copy-Item x CLAUDE.md -Force")));
        assert!(confirme(&ps("\"texte\" | Out-File C:\\x.txt")));
        assert!(confirme(&ps("Get-Content a | Tee-Object b")));
        assert!(confirme(&ps("New-Item -ItemType File x.txt")));
        // H3 : interpréteurs et sous-shells.
        assert!(confirme(&bash("python -c \"open(r'scripts\\x.py','w').write('y')\"")));
        assert!(confirme(&bash("python - <<'PY'\nimport os\nPY")));
        assert!(confirme(&bash("cmd /c \"del x\"")));
        assert!(confirme(&bash("cmd /c \"echo l'eau & del x\"")));
        assert!(confirme(&bash("bash -c 'rm -rf x'")));
        assert!(confirme(&bash("echo $(rm x)")));
        assert!(confirme(&bash("echo \"$(rm x)\"")));
        assert!(confirme(&bash("echo `rm x`")));
        assert!(confirme(&bash("powershell -Command \"Remove-Item C:\\x\"")));
        assert!(confirme(&bash("powershell -EncodedCommand ZQBjAGgAbwA=")));
        assert!(confirme(&ps("Invoke-Expression $code")));
        assert!(confirme(&ps("& 'C:\\x\\y.exe' --z")));
        // H4 : un sous-agent.
        assert!(confirme(&examiner("Agent", &json!({ "description": "inscrire", "prompt": "…" }))));
        // M1 : éteindre le poste.
        assert!(confirme(&ps("Stop-Computer -Force")));
        assert!(confirme(&ps("Restart-Computer")));
        // M2 : écritures HTTP.
        assert!(confirme(&bash("curl -XPOST https://x.ca -d y")));
        assert!(confirme(&bash("curl -X POST https://exemple.ca/api -d '{}'")));
        assert!(confirme(&bash("curl -T fichier https://x.ca")));
        assert!(confirme(&bash("curl -o sortie.html https://x.ca")));
        assert!(confirme(&ps("irm https://x.ca -Method 'Post' -Body $b")));
        assert!(confirme(&ps("iwr -Uri https://x.ca -Me Put")));
        // M3 : supprimer par COM.
        assert!(confirme(&ps("$mail.Delete()")));
        assert!(confirme(&ps("$o = New-Object -ComObject Outlook.Application")));
        // M4 : SELECT … INTO.
        let sql = |q: &str| examiner("mcp__entrepreneur-general__sql", &json!({ "requete": q }));
        assert!(confirme(&sql("SELECT * INTO copie FROM compta_factures")));
        // B1 : un dossier de projet créé sans --ecrire.
        assert!(confirme(&bash("python scripts/nouveau_projet.py creer \"Toitures Exemple inc.\"")));
    }

    #[test]
    fn fichiers_et_poste() {
        assert!(confirme(&bash("rm -rf /tmp/x")));
        assert!(confirme(&bash("cd x && del fichier.txt")));
        assert!(confirme(&ps("Remove-Item C:\\x -Recurse")));
        assert!(confirme(&ps("Get-ChildItem *.tmp | % Delete")));
        assert!(confirme(&ps("Get-ChildItem | ForEach-Object { Remove-Item $_ }")));
        assert!(confirme(&ps("foreach ($f in $l) { del $f }")));
        assert!(confirme(&ps("[System.IO.File]::WriteAllText('x', 'y')")));
        assert!(confirme(&ps("Add-Type -AssemblyName Microsoft.VisualBasic")));
        assert!(confirme(&bash("python scripts/quitter.py ranger --fermer")));
        assert!(confirme(&bash("python scripts/outlook_mail.py delete --id ABC")));
        assert!(confirme(&bash("python C:/temp/x.py")));
        assert!(confirme(&bash("find . -name '*.tmp' -delete")));
        assert!(confirme(&bash("sed -i 's/a/b/' f.txt")));
        assert!(confirme(&bash("git push")));
        assert!(confirme(&bash("ls | xargs rm")));
        assert!(confirme(&bash("inconnu --fais-quelque-chose")));
        assert!(confirme(&examiner("Edit", &json!({ "file_path": "CLAUDE.md" }))));
    }

    #[test]
    fn les_secrets_ne_se_lisent_pas() {
        assert!(refuse(&ps("Get-Content $env:LOCALAPPDATA\\Adjointe\\.env")));
        assert!(refuse(&bash("cat $LOCALAPPDATA/Adjointe/.env")));
        assert!(refuse(&bash("echo $EG_JETON")));
        assert!(refuse(&bash("python scripts/adjointe_config.py --jeton-mcp")));
        assert!(refuse(&examiner("Read", &json!({ "file_path": "C:/Users/x/AppData/Local/Adjointe/.env" }))));
        assert!(refuse(&examiner("Read", &json!({ "file_path": "C:/x/acces.env" }))));
        assert!(confirme(&bash("env")));
        assert!(confirme(&ps("Get-ChildItem env:")));
        assert_eq!(bash("python scripts/installer_jeton_base.py --verifier"), Decision::Libre);
        assert!(confirme(&bash("python scripts/installer_jeton_base.py")));
    }

    #[test]
    fn sql_et_api() {
        let sql = |q: &str| examiner("mcp__entrepreneur-general__sql", &json!({ "requete": q }));
        assert_eq!(sql("SELECT count(*) FROM compta_factures WHERE note LIKE '%update%'"), Decision::Libre);
        assert_eq!(sql("-- delete plus tard\nSELECT 1"), Decision::Libre);
        assert!(confirme(&sql("UPDATE compta_factures SET verifie = 'oui' WHERE id = 3")));
        assert!(confirme(&sql("WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x")));
        assert!(confirme(&sql("DO $$ BEGIN PERFORM 1; END $$")));
        let api =
            |m: &str| examiner("mcp__entrepreneur-general__api", &json!({ "methode": m, "chemin": "/api/changes" }));
        assert_eq!(api("GET"), Decision::Libre);
        assert!(confirme(&api("POST")));
        assert_eq!(examiner("mcp__entrepreneur-general__schema", &json!({})), Decision::Libre);
    }

    #[test]
    fn autres_services() {
        assert!(confirme(&examiner("mcp__claude_ai_Render__trigger_deploy", &json!({}))));
        assert_eq!(examiner("mcp__claude_ai_Render__list_services", &json!({})), Decision::Libre);
    }

    #[test]
    fn un_detail_ecourte_le_dit() {
        let long = format!("python scripts/compta_base.py inscrire --ecrire --note \"{}\"", "x".repeat(2500));
        match bash(&long) {
            Decision::Confirmer { detail, .. } => assert!(detail.starts_with("⚠ Commande écourtée")),
            autre => panic!("{autre:?}"),
        }
    }

    #[test]
    fn ecourter_ne_coupe_pas_un_caractere() {
        assert_eq!(ecourter("éàü", 2), "éà […]");
        assert_eq!(ecourter("court", 10), "court");
    }

    /// Depuis le 24 octobre 2026, un spécialiste a d'office la liberté totale : tout outil, tout serveur, Constructo AI.
    #[test]
    fn un_specialiste_a_la_liberte_totale() {
        let e = Espace::specialiste();
        for permis in [
            "Read",
            "Bash",
            "PowerShell",
            "Agent",
            "Task",
            "Skill",
            "ToolSearch",
            "mcp__claude_ai_Constructo_AI__enregistrer_paiement",
            "mcp__claude-in-chrome__navigate",
            "mcp__freecad__freecad_exec",
            "mcp__constructo-ai__creer_facture",
        ] {
            assert_eq!(e.refus(permis), None, "{permis}");
        }
        assert!(e.accords, "les accords à la voix restent d'office");
        assert_eq!(e.outils_de_claude_code(), None, "tous les outils de Claude Code : pas de liste --tools");
        assert!(e.recherche_d_outils());
    }

    #[test]
    fn l_espace_d_un_specialiste_borne() {
        let e = Espace::borne();
        for permis in ["Read", "Write", "Edit", "Glob", "Grep", "WebSearch", "WebFetch", "read"] {
            assert_eq!(e.refus(permis), None, "{permis}");
        }
        for hors in [
            "Bash",
            "PowerShell",
            "Agent",
            "Task",
            "NotebookEdit",
            "Skill",
            "ToolSearch",
            "",
            "mcp__claude_ai_Constructo_AI__enregistrer_paiement",
            "mcp__claude-in-chrome__navigate",
            "mcp__freecad__freecad_exec",
            "mcp__entrepreneur-general__sql",
            "mcp__constructo-ai__lister_factures",
        ] {
            assert!(e.refus(hors).is_some(), "{hors}");
        }
        assert!(e.refus("Bash").unwrap().contains("reglages.json"));
    }

    #[test]
    fn l_adjointe_garde_ce_qu_elle_avait() {
        let e = Espace::adjointe();
        for permis in
            ["Bash", "PowerShell", "Agent", "Read", "ToolSearch", "mcp__entrepreneur-general__sql", "mcp__freecad__x"]
        {
            assert_eq!(e.refus(permis), None, "{permis}");
        }
        assert!(e.refus("mcp__claude_ai_Constructo_AI__lister_projets").is_some(), "ni connecteur");
        assert!(e.refus("mcp__claude-in-chrome__navigate").is_some(), "ni Chrome");
        assert!(e.refus("mcp__constructo-ai__lister_projets").is_some(), "ni Constructo AI");
        assert!(e.accords);
    }

    #[test]
    fn chaque_privilege_ouvre_sa_porte() {
        let mut e = Espace::borne();
        e.connecteurs = true;
        assert_eq!(e.refus("mcp__claude_ai_Constructo_AI__lister_projets"), None);
        assert_eq!(e.refus("ToolSearch"), None, "la recherche d'outils va avec les serveurs MCP");
        assert!(e.refus("mcp__freecad__freecad_exec").is_some(), "les connecteurs n'ouvrent pas les serveurs du poste");
        assert!(e.refus("mcp__constructo-ai__lister_projets").is_some(), "ni la connexion d'Eve à Constructo AI");
        e.chrome = true;
        assert_eq!(e.refus("mcp__claude-in-chrome__navigate"), None);
        e.outils.as_mut().unwrap().push("PowerShell".into());
        assert_eq!(e.refus("PowerShell"), None);
        assert!(e.refus("Bash").is_some());
        let mut c = Espace::borne();
        c.constructo = true;
        assert_eq!(c.refus("mcp__constructo-ai__lister_projets"), None);
        assert_eq!(c.refus("ToolSearch"), None);
        assert!(
            c.refus("mcp__claude_ai_Constructo_AI__lister_projets").is_some(),
            "Constructo AI n'ouvre pas les connecteurs"
        );
        assert!(c.refus("mcp__freecad__freecad_exec").is_some());
        assert_eq!(
            Espace::borne().resume(),
            "les outils Read, Write, Edit, Glob, Grep, WebSearch, WebFetch ; sans les connecteurs de claude.ai ; sans \
             Chrome ; sans les serveurs MCP du poste ; sans Constructo AI ; avec les accords à la voix (« je confirme » \
             avant une suppression demandée à la voix et toute écriture dans Constructo AI) ; les jetons et les .env, \
             jamais"
        );
        let libre = Espace { accords: false, ..Espace::specialiste() };
        assert!(libre.resume().starts_with("tous les outils ; avec les connecteurs de claude.ai ; avec Chrome ; "));
        assert!(libre.resume().contains("avec Constructo AI ; sans accord : tout part sans « je confirme »"));
    }

    /// Une liste d'outils qui omet `ToolSearch` couperait la recherche d'outils : avec Constructo AI ou les connecteurs,
    /// Claude Code chargerait alors leurs centaines d'outils à chaque tour.
    #[test]
    fn la_recherche_d_outils_reste_avec_les_serveurs_nombreux() {
        let borne = Espace::borne();
        assert!(!borne.recherche_d_outils());
        assert_eq!(borne.outils_de_claude_code().unwrap(), OUTILS_BORNES.to_vec(), "la liste telle quelle");
        let avec = Espace { constructo: true, ..Espace::borne() };
        let liste = avec.outils_de_claude_code().unwrap();
        assert_eq!(liste.last().map(String::as_str), Some("ToolSearch"));
        assert_eq!(liste.iter().filter(|o| o.as_str() == "ToolSearch").count(), 1);
        let deja =
            Espace { connecteurs: true, outils: Some(vec!["Read".into(), "toolsearch".into()]), ..Espace::borne() };
        assert_eq!(deja.outils_de_claude_code().unwrap(), ["Read", "toolsearch"], "déjà là, pas en double");
        let chrome = Espace { chrome: true, ..Espace::borne() };
        assert!(!chrome.recherche_d_outils(), "Chrome seul garde ses outils chargés d'emblée");
    }

    // ---- Constructo AI ---------------------------------------------------------------------------------------------

    fn constructo(nom: &str, entree: Value) -> Option<Decision> {
        accord_erp(&format!("mcp__{SERVEUR_CONSTRUCTO}__{nom}"), &entree)
    }

    /// Ce qui se dit à voix haute, après « J'ai besoin de ton accord pour ».
    fn dit(d: Option<Decision>) -> String {
        match d {
            Some(Decision::Confirmer { quoi, .. }) => quoi,
            autre => panic!("{autre:?}"),
        }
    }

    #[test]
    fn les_deux_serveurs_de_l_erp_se_reconnaissent() {
        assert_eq!(outil_erp("mcp__constructo-ai__creer_facture"), Some("creer_facture"));
        assert_eq!(outil_erp("mcp__claude_ai_Constructo_AI__creer_facture"), Some("creer_facture"));
        assert_eq!(outil_erp("mcp__claude_ai_constructo__lister_projets"), Some("lister_projets"));
        for autre in
            ["mcp__claude_ai_Gmail__send", "mcp__freecad__freecad_exec", "creer_facture", "mcp__constructo-ai__"]
        {
            assert_eq!(outil_erp(autre), None, "{autre}");
        }
    }

    /// Les outils de Constructo AI ont des noms français : la règle générale des serveurs MCP (des verbes anglais) les
    /// laissait tous partir sans accord. Les écritures qui coûtent le plus cher, une à une, par les deux serveurs.
    #[test]
    fn les_ecritures_de_constructo_attendent_l_accord() {
        for nom in [
            "creer_facture",
            "modifier_devis",
            "enregistrer_paiement",
            "ajuster_stock",
            "envoyer_message",
            "cao_estimer_document",
            "comptabilite_executer_script",
            "banque_publier_operation",
            "soumettre_demande_subvention",
            "pointer_par_code",
            "email_deplacer_message",
            "generer_alertes",
            "un_outil_que_le_serveur_ajoutera_demain",
        ] {
            for serveur in ["mcp__constructo-ai__", "mcp__claude_ai_Constructo_AI__"] {
                let outil = format!("{serveur}{nom}");
                assert!(confirme(&accord_erp(&outil, &json!({ "x": 1 })).unwrap()), "{outil}");
                assert!(confirme(&examiner(&outil, &json!({ "x": 1 }))), "l'essai d'Eve aussi : {outil}");
            }
        }
        // Le détail affiché : l'outil et son entrée entière.
        match constructo("supprimer_projet", json!({ "project_id": "7" })) {
            Some(Decision::Confirmer { quoi, detail }) => {
                assert_eq!(quoi, "supprimer le projet dans Constructo AI");
                assert!(
                    detail.starts_with("supprimer_projet\n") && detail.contains("\"project_id\":\"7\""),
                    "{detail}"
                );
            }
            autre => panic!("{autre:?}"),
        }
    }

    #[test]
    fn les_lectures_de_constructo_partent() {
        for nom in [
            "lister_factures", "obtenir_projet", "tableau_de_bord", "statistiques_logistique", "banque_lister_comptes",
            "suivi_etat_kanban", "whoami_tenant", "decrire_tables", "fiche_360_entreprise", "calculer_beton",
            "cao_quantites", "config_lire_infos_employeur", "obtenir_previsions_meteo",
            // Un brouillon ne part pas : la personne l'envoie de l'ERP.
            "email_creer_brouillon",
        ] {
            assert_eq!(constructo(nom, json!({})), None, "{nom}");
            assert_eq!(examiner(&format!("mcp__claude_ai_Constructo_AI__{nom}"), &json!({})), Decision::Libre, "{nom}");
        }
        // Le verbe compte dans les deux premiers segments seulement (`mcp_profils._verbe_en_tete`).
        assert!(constructo("magasin_stock_lister_lots", json!({})).is_some());
    }

    #[test]
    fn la_recherche_sql_de_constructo_lit_seulement() {
        let sql = |q: &str| constructo("recherche_sql", json!({ "requete_sql": q }));
        assert_eq!(sql("SELECT numero, montant_total FROM factures WHERE statut = 'EN_RETARD'"), None);
        assert_eq!(dit(sql("UPDATE factures SET statut = 'PAYEE'")), "écrire dans Constructo AI, en SQL");
        assert!(sql("WITH x AS (DELETE FROM devis RETURNING id) SELECT * FROM x").is_some());
        // Le SQL d'administration n'est plus refusé d'office (le 24 octobre 2026 : aucune contrainte) : il se
        // confirme, même pour lire — il voit au-delà de l'entreprise.
        let admin = constructo("sql_admin", json!({ "requete_sql": "SELECT 1", "schema": "public" }));
        assert_eq!(dit(admin), "lancer du SQL d'administration dans Constructo AI");
    }

    /// Ce que la personne entend dit le geste et ce qu'il touche ; un paiement dit son montant.
    #[test]
    fn l_accord_dit_ce_qui_va_etre_fait() {
        let rien = || json!({});
        for (nom, entree, attendu) in [
            ("creer_facture", rien(), "créer une facture dans Constructo AI"),
            ("modifier_devis", rien(), "modifier le devis dans Constructo AI"),
            ("supprimer_facture", rien(), "supprimer la facture dans Constructo AI"),
            (
                "enregistrer_paiement",
                json!({ "invoice_id": 3, "montant": 500 }),
                "enregistrer un paiement de 500 dollars dans Constructo AI",
            ),
            (
                "enregistrer_paiement",
                json!({ "montant": 5214.9 }),
                "enregistrer un paiement de 5214,90 dollars dans Constructo AI",
            ),
            ("enregistrer_paiement", rien(), "enregistrer un paiement dans Constructo AI"),
            ("ajouter_ligne_facture", rien(), "ajouter une ligne de facture dans Constructo AI"),
            ("assigner_employe_bt", rien(), "assigner un employé au bon de travail dans Constructo AI"),
            ("desassigner_employe_bt", rien(), "désassigner un employé du bon de travail dans Constructo AI"),
            ("changer_statut_bt", rien(), "changer le statut du bon de travail dans Constructo AI"),
            ("envoyer_message", rien(), "envoyer un message dans Constructo AI"),
            ("banque_publier_operation", rien(), "publier l'opération dans la banque de Constructo AI"),
            ("email_supprimer_message", rien(), "supprimer le message dans les courriels de Constructo AI"),
            ("email_marquer_lu", rien(), "marquer comme lu dans les courriels de Constructo AI"),
            ("pointer_par_code", rien(), "pointer par code dans Constructo AI"),
            ("cao_ajouter_velux", rien(), "ajouter « velux » dans une maquette de Constructo AI"),
            ("quickbooks_non_transmis", rien(), "écrire dans Constructo AI (quickbooks non transmis)"),
        ] {
            assert_eq!(dit(constructo(nom, entree)), attendu, "{nom}");
        }
    }

    /// La copie des verbes de lecture, confrontée au serveur outil par outil : `essais/outils_constructo.json` porte
    /// chaque outil réellement exposé et ce qu'en dit `mcp_profils.est_lecture` (régénéré dans le dépôt de Constructo
    /// AI par `EVE/essais/outils_constructo.py`, puis recopié ici). Il décrit le serveur de l'ERP : il reste sur le poste
    /// où l'on développe Eve, hors du kit et du dépôt public ; sans lui, l'essai ne se fait pas.
    #[test]
    fn les_outils_constructo_comme_le_serveur() {
        let chemin = concat!(env!("CARGO_MANIFEST_DIR"), "/essais/outils_constructo.json");
        let Ok(texte) = std::fs::read_to_string(chemin) else {
            eprintln!("{chemin} n'est pas là (il ne part pas avec les sources) : l'essai ne se fait pas");
            return;
        };
        let serveur: Value = serde_json::from_str(&texte).unwrap();
        // Les listes d'abord, à l'identique : un verbe qu'aucun outil n'emploie encore seul ne change aucun classement
        // d'aujourd'hui, et sa copie pourrait dériver sans qu'un outil le montre.
        let noms = |cle: &str| -> Vec<String> {
            serveur[cle].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
        };
        assert_eq!(noms("verbes_lecture"), VERBES_LECTURE_CONSTRUCTO.to_vec());
        let mut explicites: Vec<&str> = LECTURES_CONSTRUCTO.to_vec();
        explicites.sort_unstable();
        assert_eq!(noms("lectures_explicites"), explicites);
        let liste = serveur["outils"].as_array().unwrap();
        assert!(liste.len() > 400, "la liste du serveur est-elle complète ? {}", liste.len());
        let mut ecarts = Vec::new();
        for o in liste {
            let nom = o["nom"].as_str().unwrap();
            let lecture = o["lecture"].as_bool().unwrap();
            if est_lecture_constructo(nom) != lecture {
                ecarts.push(nom.to_string());
            }
            // Une lecture du serveur part ; tout le reste demande l'accord — sauf les deux cas nommés.
            let d = constructo(nom, json!({}));
            let attendu_libre = lecture || matches!(nom, "email_creer_brouillon" | "recherche_sql");
            if attendu_libre {
                assert_eq!(d, None, "{nom}");
            } else {
                let quoi = dit(d);
                assert!(quoi.contains("Constructo AI") && !quoi.contains('_'), "{nom} : {quoi}");
            }
        }
        assert!(ecarts.is_empty(), "la garde et le serveur divergent : {ecarts:?}");
    }

    /// Les accords gardés (d'office) : ce qui ne se défait pas attend « je confirme » — une écriture dans Constructo AI,
    /// dite ou écrite ; une suppression, dite seulement. Le reste part, et les jetons ne se lisent jamais.
    #[test]
    fn avec_les_accords_ce_qui_ne_se_defait_pas_se_confirme() {
        let e = Espace::specialiste();
        assert!(e.accords);
        let facture = json!({ "client_company_id": 4, "lignes_json": "[]" });
        let effacer = json!({ "command": "Remove-Item 'C:/x/devis.pdf'" });
        for a_la_voix in [true, false] {
            for outil in ["mcp__constructo-ai__creer_facture", "mcp__claude_ai_Constructo_AI__creer_facture"] {
                match decider(&e, outil, &facture, a_la_voix) {
                    Decision::Confirmer { quoi, .. } => assert_eq!(quoi, "créer une facture dans Constructo AI"),
                    autre => panic!("{outil} : {autre:?}"),
                }
            }
            assert_eq!(decider(&e, "mcp__constructo-ai__lister_factures", &json!({}), a_la_voix), Decision::Libre);
            assert_eq!(decider(&e, "Write", &json!({ "file_path": "C:/x/note.md" }), a_la_voix), Decision::Libre);
            assert!(refuse(&decider(&e, "Read", &json!({ "file_path": "C:/x/.env" }), a_la_voix)));
        }
        assert!(confirme(&decider(&e, "PowerShell", &effacer, true)), "une suppression dite");
        assert_eq!(decider(&e, "PowerShell", &effacer, false), Decision::Libre, "une suppression écrite part");
        assert!(confirme(&decider(&e, "Agent", &json!({ "prompt": "Supprime les doublons du dossier" }), true)));
    }

    /// Les accords retirés (le 24 octobre 2026 : « aucune confirmation ») : rien ne se confirme plus, mais
    /// l'accord qui aurait été demandé se connaît encore (le journal le note), et les refus restent.
    #[test]
    fn sans_les_accords_tout_part_sauf_les_secrets() {
        let e = Espace { accords: false, ..Espace::specialiste() };
        let facture = json!({ "client_company_id": 4, "lignes_json": "[]" });
        let effacer = json!({ "command": "Remove-Item 'C:/x/devis.pdf'" });
        for outil in ["mcp__constructo-ai__enregistrer_paiement", "mcp__claude_ai_Constructo_AI__supprimer_projet"] {
            assert_eq!(decider(&e, outil, &facture, true), Decision::Libre, "{outil}");
            assert!(accord_voulu(outil, &facture, true).is_some(), "le journal le note : {outil}");
        }
        assert_eq!(decider(&e, "PowerShell", &effacer, true), Decision::Libre);
        assert!(accord_voulu("PowerShell", &effacer, true).is_some());
        assert_eq!(decider(&e, "Agent", &json!({ "prompt": "Supprime les doublons" }), true), Decision::Libre);
        // Les jetons et les clés : refusés dans les deux cas.
        assert!(refuse(&decider(&e, "Read", &json!({ "file_path": "C:/x/.env" }), true)));
        assert!(refuse(&decider(&e, "Read", &json!({ "file_path": "C:/x/connexion.bin" }), false)));
        assert!(refuse(&decider(&e, "Bash", &json!({ "command": "echo $EG_JETON" }), false)));
        // L'espace d'actions aussi : un privilège retiré se refuse, accords ou non.
        let sans_constructo = Espace { constructo: false, ..e };
        assert!(refuse(&decider(&sans_constructo, "mcp__constructo-ai__lister_factures", &json!({}), false)));
    }

    #[test]
    fn la_connexion_de_constructo_ne_se_lit_pas() {
        let lire = |p: &str| evaluer("Read", &json!({ "file_path": p }));
        let fichier = r"C:\Users\x\AppData\Local\Eve\specialistes\assistante\connexion.bin";
        assert!(refuse(&lire(fichier)));
        assert!(refuse(&evaluer(
            "PowerShell",
            &json!({ "command": format!("Get-Content '{fichier}' -Encoding Byte") })
        )));
        assert!(refuse(&evaluer("Grep", &json!({ "pattern": "acces", "path": fichier }))));
        assert_eq!(
            lire(r"C:\Users\x\AppData\Local\Eve\specialistes\assistante\pieces\2026-10-24\facture.pdf"),
            Decision::Libre
        );
    }
}
