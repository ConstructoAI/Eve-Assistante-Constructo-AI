//! Le protocole entre Eve et Claude Code.
//!
//! Eve lance `claude -p --input-format stream-json --output-format stream-json --verbose` : un message JSON par
//! ligne dans chaque sens. C'est le protocole qu'emploient les SDK officiels d'Anthropic (claude-agent-sdk, en Python
//! et en TypeScript) ; les formes ci-dessous en sont reprises (`_internal/query.py`, `client.py`, octobre 2026).
//!
//! **Vers Claude Code** : un message de la personne (`{"type":"user",…}`) ou une demande de contrôle
//! (`{"type":"control_request","request_id":…,"request":{"subtype":…}}`) — l'initialisation, qui branche le crochet de
//! la garde, et l'interruption.
//!
//! **De Claude Code** : `system` (l'ouverture : la session, le modèle, l'état des serveurs MCP), `assistant` (le texte
//! et les appels d'outils), `user` (les résultats d'outils), `result` (la fin d'un tour, avec la réponse), les réponses
//! à nos demandes (`control_response`), et ses propres demandes (`control_request`) : `hook_callback` (le crochet de la
//! garde, avant chaque outil), `can_use_tool` (une permission), auxquelles Eve répond par `control_response`. Avec
//! `--include-partial-messages`, la réponse arrive aussi mot à mot (`stream_event`) : Eve la dit phrase par phrase
//! pendant qu'elle s'écrit ([`Flux`]).

use serde_json::{Value, json};

/// L'identifiant du crochet de la garde, que Claude Code rappelle (`callback_id`) avant chaque outil.
pub const CROCHET_GARDE: &str = "garde";

/// Le délai que Claude Code laisse au crochet (en secondes) : le temps de dire « je confirme ».
pub const DELAI_CROCHET: u64 = 600;

/// Un message de la personne.
pub fn message(texte: &str) -> String {
    json!({
        "type": "user",
        "message": { "role": "user", "content": texte },
        "parent_tool_use_id": null,
        "session_id": "default",
    })
    .to_string()
}

/// Une demande de contrôle vers Claude Code.
pub fn demande(id: &str, demande: Value) -> String {
    json!({ "type": "control_request", "request_id": id, "request": demande }).to_string()
}

/// L'initialisation : le crochet `PreToolUse` de la garde, sur tous les outils (`matcher` nul).
pub fn initialisation() -> Value {
    json!({
        "subtype": "initialize",
        "hooks": {
            "PreToolUse": [ { "matcher": null, "hookCallbackIds": [CROCHET_GARDE], "timeout": DELAI_CROCHET } ]
        }
    })
}

/// L'interruption du tour en cours.
pub fn interruption() -> Value {
    json!({ "subtype": "interrupt" })
}

/// L'effort de réflexion des tours qui suivent (`apply_flag_settings`, la clé `effortLevel` : « low », « medium »,
/// « high », « xhigh », « max ») — le même que donne `--effort` au lancement, que Claude Code 2.1.312 applique avant de
/// lire le message suivant (vérifié par `get_settings`, `applied.effort`). Quand l'effort se donne tour par tour
/// (`per_turn_effort_active` de l'ouverture), le changer garde le prompt en cache : Claude Code le glisse dans la
/// conversation, au tour où il change ; sinon, il récrit tout le préfixe en cache. (Un effort nul ne revient pas à
/// celui du lancement, mais à celui du modèle : on redonne toujours l'effort voulu.)
pub fn effort(niveau: &str) -> Value {
    json!({ "subtype": "apply_flag_settings", "settings": { "effortLevel": niveau } })
}

/// L'état des serveurs MCP (`mcp_status`) : la réponse (`{"mcpServers": [{"name", "status"}]}`) dit si la base est
/// branchée — Eve attend qu'elle le soit pour préchauffer : ses outils font partie du contexte mis en cache.
pub fn etat_mcp() -> Value {
    json!({ "subtype": "mcp_status" })
}

/// Le statut d'un serveur MCP dans la réponse à [`etat_mcp`] (« connected », « pending », « failed »…).
pub fn statut_mcp<'a>(corps: &'a Value, serveur: &str) -> Option<&'a str> {
    corps
        .get("mcpServers")
        .and_then(Value::as_array)?
        .iter()
        .find(|s| texte(s, "name") == serveur)
        .map(|s| texte(s, "status"))
}

/// La réponse réussie à une demande de Claude Code.
pub fn reponse(id: &str, corps: Value) -> String {
    json!({ "type": "control_response", "response": { "subtype": "success", "request_id": id, "response": corps } })
        .to_string()
}

/// La réponse en erreur à une demande de Claude Code.
pub fn reponse_erreur(id: &str, erreur: &str) -> String {
    json!({ "type": "control_response", "response": { "subtype": "error", "request_id": id, "error": erreur } })
        .to_string()
}

/// La sortie du crochet qui laisse l'outil suivre son cours (aucune décision).
pub fn crochet_neutre() -> Value {
    json!({})
}

/// La sortie du crochet qui laisse partir l'outil, accordé par la personne.
pub fn crochet_accord(raison: &str) -> Value {
    json!({ "hookSpecificOutput": {
        "hookEventName": "PreToolUse", "permissionDecision": "allow", "permissionDecisionReason": raison } })
}

/// La sortie du crochet qui arrête l'outil ; la raison revient à l'Adjointe.
pub fn crochet_refus(raison: &str) -> Value {
    json!({ "hookSpecificOutput": {
        "hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": raison } })
}

/// Une permission accordée (`can_use_tool`) : l'entrée part telle quelle.
pub fn permission_accord(entree: &Value) -> Value {
    json!({ "behavior": "allow", "updatedInput": entree })
}

/// Une permission refusée (`can_use_tool`).
pub fn permission_refus(message: &str) -> Value {
    json!({ "behavior": "deny", "message": message })
}

/// Un morceau de la réponse en train de s'écrire (`--include-partial-messages`) : les événements de l'API que Claude
/// Code relaie (`stream_event`), pour le fil principal seulement (pas ceux d'un sous-agent).
#[derive(Debug, PartialEq, Eq)]
pub enum Flux {
    /// Un nouveau message de l'Adjointe commence.
    Debut,
    /// Un bloc de texte commence.
    DebutTexte,
    /// Du texte s'ajoute au bloc en cours.
    Texte(String),
    /// Un appel d'outil commence (son nom).
    Outil(String),
    /// Le bloc en cours se termine.
    FinBloc,
}

/// Un message reçu de Claude Code, rangé.
#[derive(Debug)]
pub enum Recu<'a> {
    /// L'ouverture de la session (à chaque tour) : les serveurs MCP et leur état, les outils offerts ; et si l'effort
    /// se donne tour par tour (`per_turn_effort_active` ; rien : on ne sait pas), ce qui permet de le changer sans
    /// perdre le cache ([`effort`]).
    Ouverture {
        session: String,
        modele: String,
        serveurs: Vec<(String, String)>,
        outils: Vec<String>,
        effort_par_tour: Option<bool>,
    },
    /// L'effort ne se donne plus tour par tour (`system/per_turn_effort_changed`, toujours à faux : le serveur a refusé
    /// ce message) : le changer récrirait désormais le préfixe en cache.
    EffortFixe,
    /// Un message de l'Adjointe : son texte et ses appels d'outils (`(identifiant, nom, entrée)`) ; `panne` : la sorte
    /// d'erreur de l'API qu'il porte (son champ `error` : « authentication_failed », « rate_limit »…), vide sans elle.
    Adjointe { textes: Vec<String>, outils: Vec<(String, String, &'a Value)>, panne: String },
    /// Les résultats d'outils : leur identifiant, et s'ils sont en erreur. Un outil que Claude Code rejette avant même
    /// le crochet de la garde (une entrée invalide, un outil annulé) revient en erreur sans avoir tourné.
    Resultats { outils: Vec<(String, bool)> },
    /// Un morceau de la réponse en train de s'écrire.
    Flux(Flux),
    /// La fin d'un tour.
    Fin { texte: String, erreur: bool, cout: Option<f64>, duree_ms: Option<u64> },
    /// Une demande de Claude Code, à laquelle il faut répondre.
    Demande { id: String, sorte: String, corps: &'a Value },
    /// La réponse à une de nos demandes, et son contenu.
    Reponse { id: String, reussie: bool, erreur: String, corps: &'a Value },
    /// Claude Code abandonne une de ses demandes (le tour a été interrompu).
    Abandon { id: String },
    /// La limite d'utilisation du compte (`rate_limit_event`) : son état (« allowed », « allowed_warning »,
    /// « rejected »), l'instant où elle se rouvre (secondes Unix), sa sorte (« five_hour », « seven_day »…), la part
    /// utilisée (de 0 à 1).
    Limite { etat: String, reprise: Option<i64>, sorte: String, part: Option<f64> },
    /// Claude Code réessaie un appel à l'API (`system/api_retry`) : la tentative, le nombre permis, la sorte d'erreur
    /// (« overloaded », « rate_limit »…), et s'il n'a reçu aucune réponse du tout (le réseau).
    Reessai { tentative: u64, permis: u64, panne: String, sans_reponse: bool },
    /// Les agents que Claude fait travailler en arrière-plan, chacun par sa description (`background_tasks_changed`,
    /// à chaque agent qui part ou qui finit) ; vide : tous ont fini.
    Agents(Vec<String>),
    /// Le reste (les autres messages système, les événements partiels).
    Autre,
}

fn texte<'a>(v: &'a Value, cle: &str) -> &'a str {
    v.get(cle).and_then(Value::as_str).unwrap_or("")
}

/// Range un message reçu de Claude Code.
pub fn lire(v: &Value) -> Recu<'_> {
    match texte(v, "type") {
        "system" if texte(v, "subtype") == "background_tasks_changed" => Recu::Agents(
            v.get("tasks")
                .and_then(Value::as_array)
                .map(|l| l.iter().map(|t| texte(t, "description").to_string()).collect())
                .unwrap_or_default(),
        ),
        "system" if texte(v, "subtype") == "api_retry" => Recu::Reessai {
            tentative: v.get("attempt").and_then(Value::as_u64).unwrap_or(0),
            permis: v.get("max_retries").and_then(Value::as_u64).unwrap_or(0),
            panne: texte(v, "error").to_string(),
            sans_reponse: v.get("no_response").is_some_and(|r| !r.is_null()),
        },
        "rate_limit_event" => {
            let l = v.get("rate_limit_info").unwrap_or(&Value::Null);
            Recu::Limite {
                etat: texte(l, "status").to_string(),
                reprise: l.get("resetsAt").and_then(Value::as_i64),
                sorte: texte(l, "rateLimitType").to_string(),
                part: l.get("utilization").and_then(Value::as_f64),
            }
        }
        "system" if texte(v, "subtype") == "per_turn_effort_changed" => Recu::EffortFixe,
        "system" if texte(v, "subtype") == "init" => Recu::Ouverture {
            session: texte(v, "session_id").to_string(),
            modele: texte(v, "model").to_string(),
            serveurs: v
                .get("mcp_servers")
                .and_then(Value::as_array)
                .map(|l| l.iter().map(|s| (texte(s, "name").to_string(), texte(s, "status").to_string())).collect())
                .unwrap_or_default(),
            outils: v
                .get("tools")
                .and_then(Value::as_array)
                .map(|l| l.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default(),
            effort_par_tour: v.get("per_turn_effort_active").and_then(Value::as_bool),
        },
        "assistant" => {
            let mut textes = Vec::new();
            let mut outils = Vec::new();
            for bloc in v.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
                match texte(bloc, "type") {
                    "text" => textes.push(texte(bloc, "text").to_string()),
                    "tool_use" => outils.push((
                        texte(bloc, "id").to_string(),
                        texte(bloc, "name").to_string(),
                        bloc.get("input").unwrap_or(&Value::Null),
                    )),
                    _ => {}
                }
            }
            Recu::Adjointe { textes, outils, panne: texte(v, "error").to_string() }
        }
        "user" => {
            let outils = v
                .pointer("/message/content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|b| texte(b, "type") == "tool_result")
                .map(|b| {
                    (texte(b, "tool_use_id").to_string(), b.get("is_error").and_then(Value::as_bool) == Some(true))
                })
                .collect();
            Recu::Resultats { outils }
        }
        "stream_event" if v.get("parent_tool_use_id").is_none_or(Value::is_null) => {
            lire_flux(v.get("event").unwrap_or(&Value::Null))
        }
        "result" => Recu::Fin {
            texte: texte(v, "result").to_string(),
            erreur: v.get("is_error").and_then(Value::as_bool).unwrap_or(false) || texte(v, "subtype") != "success",
            cout: v.get("total_cost_usd").and_then(Value::as_f64),
            duree_ms: v.get("duration_ms").and_then(Value::as_u64),
        },
        "control_request" => Recu::Demande {
            id: texte(v, "request_id").to_string(),
            sorte: v.pointer("/request/subtype").and_then(Value::as_str).unwrap_or("").to_string(),
            corps: v.get("request").unwrap_or(&Value::Null),
        },
        "control_response" => {
            let r = v.get("response").unwrap_or(&Value::Null);
            Recu::Reponse {
                id: texte(r, "request_id").to_string(),
                reussie: texte(r, "subtype") == "success",
                erreur: texte(r, "error").to_string(),
                corps: r.get("response").unwrap_or(&Value::Null),
            }
        }
        "control_cancel_request" => Recu::Abandon { id: texte(v, "request_id").to_string() },
        _ => Recu::Autre,
    }
}

/// Un événement de la réponse en train de s'écrire.
fn lire_flux(e: &Value) -> Recu<'_> {
    let flux = match texte(e, "type") {
        "message_start" => Flux::Debut,
        "content_block_start" => match e.pointer("/content_block/type").and_then(Value::as_str) {
            Some("text") => Flux::DebutTexte,
            Some("tool_use") => {
                Flux::Outil(e.pointer("/content_block/name").and_then(Value::as_str).unwrap_or("").to_string())
            }
            _ => return Recu::Autre,
        },
        "content_block_delta" if e.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") => {
            Flux::Texte(e.pointer("/delta/text").and_then(Value::as_str).unwrap_or("").to_string())
        }
        "content_block_stop" => Flux::FinBloc,
        _ => return Recu::Autre,
    };
    Recu::Flux(flux)
}

/// Ce sur quoi porte un outil, pour reconnaître sa destination dans le réseau des actions de la fenêtre native : le
/// chemin, le motif, la commande, l'adresse ou la recherche — jamais ce que l'outil rend —, sur une ligne, 300
/// caractères au plus ; rien pour un autre outil (un outil MCP se reconnaît à son nom).
pub fn detail_outil(nom: &str, entree: &Value) -> String {
    let detail = match nom {
        "Read" | "Write" | "Edit" | "MultiEdit" => texte(entree, "file_path").to_string(),
        "NotebookEdit" => texte(entree, "notebook_path").to_string(),
        // Ce qu'on cherche, puis où : « Noah · C:/Exemple ».
        "Glob" | "Grep" => [texte(entree, "pattern"), texte(entree, "path")]
            .into_iter()
            .filter(|t| !t.trim().is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
        "Bash" | "PowerShell" => texte(entree, "command").to_string(),
        "WebFetch" => texte(entree, "url").to_string(),
        "WebSearch" => texte(entree, "query").to_string(),
        _ => String::new(),
    };
    crate::garde::ecourter(&detail.split_whitespace().collect::<Vec<_>>().join(" "), 300)
}

/// Ce que vise un outil, pour le réseau des actions : un chemin, une adresse, une commande. Pour Glob, son dossier et
/// son motif réunis en un chemin (le réseau s'arrête au premier joker) ; pour Grep, son dossier seul : son motif est une
/// expression régulière, pas un chemin — sans dossier, c'est le dossier de travail. (Le dossier et le motif collés
/// faisaient naître un faux dossier, « Famille 2026-10-24|24 octobre », à côté du vrai.)
pub fn cible_outil(nom: &str, entree: &Value) -> String {
    match nom {
        "Glob" => {
            let (dossier, motif) = (texte(entree, "path").trim(), texte(entree, "pattern").trim());
            if dossier.is_empty() {
                motif.to_string()
            } else {
                format!("{}\\{motif}", dossier.trim_end_matches(['\\', '/']))
            }
        }
        "Grep" => texte(entree, "path").trim().to_string(),
        _ => detail_outil(nom, entree),
    }
}

/// Une ligne courte qui dit ce que fait un outil, pour l'écran (« ▸ Bash : python scripts/matin.py »).
pub fn resume_outil(nom: &str, entree: &Value) -> String {
    let detail = match nom {
        "Bash" | "PowerShell" => texte(entree, "command").to_string(),
        "Read" | "Write" | "Edit" => texte(entree, "file_path").to_string(),
        "Grep" | "Glob" => texte(entree, "pattern").to_string(),
        "mcp__entrepreneur-general__sql" => texte(entree, "requete").to_string(),
        "mcp__entrepreneur-general__api" => format!("{} {}", texte(entree, "methode"), texte(entree, "chemin")),
        "Skill" => texte(entree, "skill").to_string(),
        "Agent" => texte(entree, "description").to_string(),
        _ if crate::garde::outil_erp(nom) == Some("recherche_sql") => texte(entree, "requete_sql").to_string(),
        _ => String::new(),
    };
    let nom = match (crate::garde::outil_erp(nom), nom.strip_prefix("mcp__entrepreneur-general__")) {
        (Some(n), _) => format!("Constructo · {n}"),
        (None, Some(n)) => format!("base · {n}"),
        (None, None) => nom.to_string(),
    };
    let detail: String = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let detail = crate::garde::ecourter(&detail, 140);
    if detail.is_empty() { format!("▸ {nom}") } else { format!("▸ {nom} : {detail}") }
}

#[cfg(test)]
mod essais {
    /// Les pannes, dans les formes du flux de Claude Code 2.1.310 (relevées dans son schéma).
    #[test]
    fn les_pannes_se_lisent() {
        use super::{Recu, lire};
        let v = serde_json::json!({ "type": "rate_limit_event", "uuid": "u", "session_id": "s",
            "rate_limit_info": { "status": "rejected", "resetsAt": 1_793_000_000, "rateLimitType": "five_hour",
            "utilization": 1.0 } });
        let Recu::Limite { etat, reprise, sorte, part } = lire(&v) else { panic!("une limite") };
        assert_eq!(
            (etat.as_str(), reprise, sorte.as_str(), part),
            ("rejected", Some(1_793_000_000), "five_hour", Some(1.0))
        );
        let v = serde_json::json!({ "type": "system", "subtype": "api_retry", "attempt": 2, "max_retries": 10,
            "retry_delay_ms": 4000, "error_status": 529, "error": "overloaded" });
        let Recu::Reessai { tentative, permis, panne, sans_reponse } = lire(&v) else { panic!("un nouvel essai") };
        assert_eq!((tentative, permis, panne.as_str(), sans_reponse), (2, 10, "overloaded", false));
        let v = serde_json::json!({ "type": "system", "subtype": "api_retry", "attempt": 1, "max_retries": 10,
            "retry_delay_ms": 1000, "error_status": null, "error": "unknown",
            "no_response": { "waited_ms": 30000, "retry_wait_ms": 1000 } });
        assert!(matches!(lire(&v), Recu::Reessai { sans_reponse: true, .. }));
        let v = serde_json::json!({ "type": "assistant", "error": "authentication_failed", "session_id": "s",
            "parent_tool_use_id": null, "message": { "content": [{ "type": "text", "text": "Please run /login" }] } });
        let Recu::Adjointe { panne, textes, .. } = lire(&v) else { panic!("un message") };
        assert_eq!((panne.as_str(), textes.len()), ("authentication_failed", 1));
    }

    use super::*;

    #[test]
    fn le_message_a_la_forme_des_sdk() {
        let m: Value = serde_json::from_str(&message("Bonjour")).unwrap();
        assert_eq!(m["type"], "user");
        assert_eq!(m["message"]["role"], "user");
        assert_eq!(m["message"]["content"], "Bonjour");
        assert_eq!(m["session_id"], "default");
    }

    #[test]
    fn un_crochet_se_lit() {
        let v = json!({ "type": "control_request", "request_id": "r1",
            "request": { "subtype": "hook_callback", "callback_id": "garde",
                "input": { "tool_name": "Bash", "tool_input": { "command": "ls" } }, "tool_use_id": "t1" } });
        match lire(&v) {
            Recu::Demande { id, sorte, corps } => {
                assert_eq!(id, "r1");
                assert_eq!(sorte, "hook_callback");
                assert_eq!(corps["input"]["tool_name"], "Bash");
            }
            autre => panic!("{autre:?}"),
        }
    }

    /// Les agents en arrière-plan, tels que Claude Code 2.1.310 les dit (un essai du 24 octobre 2026).
    #[test]
    fn les_agents_en_arriere_plan_se_lisent() {
        let v = json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [
            { "task_id": "a83d", "task_type": "local_agent", "subagent_type": "general-purpose",
              "description": "Réponse OK (défaut)" },
            { "task_id": "a390", "task_type": "local_agent", "subagent_type": "general-purpose",
              "description": "Réponse OK (opus, max)" } ] });
        match lire(&v) {
            Recu::Agents(a) => assert_eq!(a, ["Réponse OK (défaut)", "Réponse OK (opus, max)"]),
            autre => panic!("{autre:?}"),
        }
        let fini = json!({ "type": "system", "subtype": "background_tasks_changed", "tasks": [] });
        assert!(matches!(lire(&fini), Recu::Agents(a) if a.is_empty()));
    }

    #[test]
    fn l_ouverture_dit_les_outils_offerts() {
        let v = json!({ "type": "system", "subtype": "init", "session_id": "s1", "model": "claude-sonnet-5-5",
            "tools": ["Read", "Write", "mcp__freecad__freecad_exec"], "mcp_servers": [{ "name": "freecad", "status": "connected" }] });
        match lire(&v) {
            Recu::Ouverture { session, outils, serveurs, effort_par_tour, .. } => {
                assert_eq!(session, "s1");
                assert_eq!(outils, ["Read", "Write", "mcp__freecad__freecad_exec"]);
                assert_eq!(serveurs, [("freecad".to_string(), "connected".to_string())]);
                assert_eq!(effort_par_tour, None, "un Claude Code d'avant ne le dit pas");
            }
            autre => panic!("{autre:?}"),
        }
    }

    /// L'effort tour par tour, tel que Claude Code 2.1.312 le dit (vu le 27 octobre 2026 avec Sonnet 5.5) et le prend.
    #[test]
    fn l_effort_se_change_tour_par_tour() {
        let v = json!({ "type": "system", "subtype": "init", "session_id": "s", "model": "claude-sonnet-5-5",
            "tools": [], "mcp_servers": [], "per_turn_effort_active": true });
        assert!(matches!(lire(&v), Recu::Ouverture { effort_par_tour: Some(true), .. }));
        let fixe = json!({ "type": "system", "subtype": "per_turn_effort_changed", "per_turn_effort_active": false,
            "uuid": "u", "session_id": "s" });
        assert!(matches!(lire(&fixe), Recu::EffortFixe));
        let d: Value = serde_json::from_str(&demande("eve_effort_1", effort("low"))).unwrap();
        assert_eq!(d["type"], "control_request");
        assert_eq!(d["request"], json!({ "subtype": "apply_flag_settings", "settings": { "effortLevel": "low" } }));
    }

    #[test]
    fn la_fin_d_un_tour_se_lit() {
        let v = json!({ "type": "result", "subtype": "success", "is_error": false, "result": "C'est fait." });
        match lire(&v) {
            Recu::Fin { texte, erreur, .. } => {
                assert_eq!(texte, "C'est fait.");
                assert!(!erreur);
            }
            autre => panic!("{autre:?}"),
        }
    }

    #[test]
    fn la_reponse_en_train_de_s_ecrire_se_lit() {
        let ev =
            |e: Value| json!({ "type": "stream_event", "event": e, "parent_tool_use_id": null, "session_id": "s" });
        assert!(matches!(lire(&ev(json!({ "type": "message_start", "message": {} }))), Recu::Flux(Flux::Debut)));
        let t = ev(
            json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": "Demain" } }),
        );
        assert!(matches!(lire(&t), Recu::Flux(Flux::Texte(ref x)) if x == "Demain"));
        let o = ev(
            json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "tool_use", "name": "Bash" } }),
        );
        assert!(matches!(lire(&o), Recu::Flux(Flux::Outil(ref n)) if n == "Bash"));
        assert!(matches!(lire(&ev(json!({ "type": "content_block_stop", "index": 0 }))), Recu::Flux(Flux::FinBloc)));
        // Ni la pensée, ni l'entrée d'un outil en train de s'écrire, ni un sous-agent ne se disent.
        let pensee =
            ev(json!({ "type": "content_block_delta", "delta": { "type": "thinking_delta", "thinking": "…" } }));
        assert!(matches!(lire(&pensee), Recu::Autre));
        let sous = json!({ "type": "stream_event", "parent_tool_use_id": "t1",
            "event": { "type": "content_block_delta", "delta": { "type": "text_delta", "text": "x" } } });
        assert!(matches!(lire(&sous), Recu::Autre));
    }

    #[test]
    fn l_etat_de_la_base_se_lit() {
        let v = json!({ "type": "control_response", "response": { "subtype": "success", "request_id": "eve_mcp_3",
            "response": { "mcpServers": [ { "name": "entrepreneur-general", "status": "pending" } ] } } });
        match lire(&v) {
            Recu::Reponse { id, reussie, corps, .. } => {
                assert_eq!(id, "eve_mcp_3");
                assert!(reussie);
                assert_eq!(statut_mcp(corps, "entrepreneur-general"), Some("pending"));
                assert_eq!(statut_mcp(corps, "autre"), None);
            }
            autre => panic!("{autre:?}"),
        }
    }

    #[test]
    fn le_detail_d_un_outil_dit_sur_quoi_il_porte() {
        assert_eq!(
            detail_outil("Read", &json!({"file_path": "C:/Exemple/Maison/PROFIL.md"})),
            "C:/Exemple/Maison/PROFIL.md"
        );
        assert_eq!(detail_outil("Grep", &json!({"path": "C:/Exemple", "pattern": "Noah"})), "Noah · C:/Exemple");
        assert_eq!(detail_outil("Glob", &json!({"pattern": "*.md"})), "*.md", "sans chemin, le motif seul");
        assert_eq!(
            detail_outil("PowerShell", &json!({"command": "Get-Date\n  | Out-String"})),
            "Get-Date | Out-String"
        );
        assert_eq!(detail_outil("WebSearch", &json!({"query": "garderie Montréal"})), "garderie Montréal");
        assert_eq!(detail_outil("mcp__claude-in-chrome__navigate", &json!({"url": "https://x"})), "");
        // Pour le réseau : le dossier de Grep seul, le dossier et le motif de Glob réunis, le reste comme le détail.
        let grep = json!({"path": "C:/Exemple/Maison", "pattern": "2026-10-24|24 octobre|vaccin"});
        assert_eq!(cible_outil("Grep", &grep), "C:/Exemple/Maison");
        assert_eq!(cible_outil("Grep", &json!({"pattern": "vaccin"})), "", "sans dossier : le dossier de travail");
        assert_eq!(cible_outil("Glob", &json!({"path": "C:/Exemple/", "pattern": "**/*.pdf"})), r"C:/Exemple\**/*.pdf");
        assert_eq!(cible_outil("Glob", &json!({"pattern": "activite/*"})), "activite/*");
        assert_eq!(cible_outil("Read", &json!({"file_path": "C:/a.md"})), "C:/a.md");
    }

    #[test]
    fn un_outil_de_constructo_se_resume() {
        assert_eq!(resume_outil("mcp__constructo-ai__lister_factures", &json!({})), "▸ Constructo · lister_factures");
        assert_eq!(
            resume_outil("mcp__claude_ai_Constructo_AI__recherche_sql", &json!({"requete_sql": "SELECT 1"})),
            "▸ Constructo · recherche_sql : SELECT 1"
        );
        assert_eq!(
            resume_outil("mcp__entrepreneur-general__sql", &json!({"requete": "SELECT 2"})),
            "▸ base · sql : SELECT 2"
        );
    }

    #[test]
    fn un_resultat_en_erreur_se_voit() {
        let v = json!({ "type": "user", "message": { "role": "user", "content": [
            { "type": "tool_result", "tool_use_id": "t1", "is_error": true, "content": "String to replace not found" },
            { "type": "tool_result", "tool_use_id": "t2", "content": "ok" } ] } });
        match lire(&v) {
            Recu::Resultats { outils } => assert_eq!(outils, vec![("t1".to_string(), true), ("t2".to_string(), false)]),
            autre => panic!("{autre:?}"),
        }
    }

    #[test]
    fn la_reponse_a_la_forme_attendue() {
        let r: Value = serde_json::from_str(&reponse("r1", crochet_refus("non"))).unwrap();
        assert_eq!(r["type"], "control_response");
        assert_eq!(r["response"]["subtype"], "success");
        assert_eq!(r["response"]["request_id"], "r1");
        assert_eq!(r["response"]["response"]["hookSpecificOutput"]["permissionDecision"], "deny");
    }
}
