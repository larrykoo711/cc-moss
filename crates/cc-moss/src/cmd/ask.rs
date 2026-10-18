//! `cc-moss ask list|answer|take`: see and answer the questions sessions
//! are waiting on. The files are the protocol (`moss_core::ask`); these
//! commands are its reference client.

use anyhow::{anyhow, bail, Result};
use std::time::{Duration, Instant};

use moss_core::ask::{
    self, Answer, AnswerError, Ask, Behavior, By, ChoiceError, Kind, Reason, Store,
};
use moss_core::slot::now_ms;
use moss_core::store_dir;
use serde_json::json;

pub const JSON_SCHEMA: u32 = 1;

pub enum Action {
    List {
        json: bool,
    },
    Answer {
        id: String,
        choices: Vec<String>,
        text: Option<String>,
        allow: bool,
        deny: bool,
        message: Option<String>,
        by: By,
        json: bool,
    },
    Take {
        name: String,
    },
}

pub fn run(action: Action) -> Result<()> {
    let store = Store::new(&store_dir::state_dir());
    match action {
        Action::List { json } => list(&store, json),
        Action::Answer {
            id,
            choices,
            text,
            allow,
            deny,
            message,
            by,
            json,
        } => {
            let reply = Reply {
                choices,
                text,
                allow,
                deny,
                message,
            };
            answer(&store, &id, &reply, by, json)
        }
        Action::Take { name } => take(&store, &name),
    }
}

fn list(store: &Store, as_json: bool) -> Result<()> {
    let now = now_ms();
    store.sweep(now);
    let asks = store.open(now);
    if as_json {
        let out = json!({ "schema": JSON_SCHEMA, "asks": asks });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    if asks.is_empty() {
        println!(
            "{}",
            tr!(
                "No session is waiting on a question.",
                "没有会话在等你回答。"
            )
        );
        return Ok(());
    }
    for a in &asks {
        let who = a
            .session_name
            .clone()
            .unwrap_or_else(|| a.session_id.chars().take(8).collect());
        match a.kind {
            Kind::Question => {
                for q in a.questions.iter().flatten() {
                    let text = q.get("question").and_then(|v| v.as_str()).unwrap_or("");
                    let labels: Vec<&str> = q
                        .get("options")
                        .and_then(|o| o.as_array())
                        .into_iter()
                        .flatten()
                        .filter_map(|o| o.get("label").and_then(|l| l.as_str()))
                        .collect();
                    let several = q.get("multiSelect").and_then(|m| m.as_bool()) == Some(true);
                    let hint = if several {
                        tr!(" (pick any)", "（可多选）")
                    } else {
                        ""
                    };
                    println!("{}  {who}  {text}{hint}", a.id);
                    println!("    {}", labels.join(" / "));
                }
            }
            Kind::Permission => {
                let (tool, preview) = a
                    .tool
                    .as_ref()
                    .map(|t| (t.name.as_str(), t.preview.as_str()))
                    .unwrap_or_default();
                println!(
                    "{}  {who}  {}",
                    a.id,
                    tr!(
                        format!("wants to use {tool}: {preview}"),
                        format!("想用 {tool}：{preview}")
                    )
                );
            }
        }
    }
    Ok(())
}

struct Reply {
    choices: Vec<String>,
    text: Option<String>,
    allow: bool,
    deny: bool,
    message: Option<String>,
}

fn build(ask: &Ask, reply: &Reply, by: By) -> Result<Answer> {
    let mut answer = Answer {
        schema: ask::SCHEMA,
        by,
        answers: None,
        behavior: None,
        message: None,
    };
    match ask.kind {
        Kind::Question => {
            if reply.allow || reply.deny {
                bail!(tr!(
                    "this is a question: answer with --choice or --text",
                    "这是一道选择题：用 --choice 或 --text 回答"
                ));
            }
            let questions = ask.questions.as_deref().unwrap_or_default();
            let answers = ask::answers_from(questions, &reply.choices, reply.text.as_deref())
                .map_err(|e| anyhow!(choice_error(&e, questions.len())))?;
            answer.answers = Some(answers);
        }
        Kind::Permission => {
            if !reply.choices.is_empty() || reply.text.is_some() {
                bail!(tr!(
                    "this asks for permission: answer with --allow or --deny",
                    "这是在请求权限：用 --allow 或 --deny 回答"
                ));
            }
            answer.behavior = Some(if reply.allow {
                Behavior::Allow
            } else {
                Behavior::Deny
            });
            answer.message = reply.message.clone().filter(|m| !m.is_empty());
        }
    }
    Ok(answer)
}

fn choice_error(e: &ChoiceError, count: usize) -> String {
    match e {
        ChoiceError::NotAnOption(label) => tr!(
            format!("`{label}` is not one of the options (see `cc-moss ask list`)"),
            format!("`{label}` 不是可选项（看看 `cc-moss ask list`）")
        ),
        ChoiceError::Unanswered(n) if count > 1 => tr!(
            format!(
                "question {n} of {count} has no answer; give --choice per question, in order, or as N:LABEL (e.g. --choice {n}:yes)"
            ),
            format!("第 {n} 题（共 {count} 题）还没答；按题目顺序给 --choice，或者写成 题号:选项（比如 --choice {n}:是）")
        ),
        ChoiceError::Unanswered(_) => tr!(
            "give an answer with --choice or --text".to_string(),
            "用 --choice 或 --text 给个答案".to_string()
        ),
        ChoiceError::TextNeedsOneQuestion => tr!(
            "--text works for a single question; this one has several".to_string(),
            "--text 只能用在只有一题的时候；这里有好几题".to_string()
        ),
        ChoiceError::OneOnly(n) => tr!(
            format!("question {n} takes one choice"),
            format!("第 {n} 题只能选一个")
        ),
    }
}

/// A permission is only granted on an id typed out this far, so a stray
/// prefix cannot approve the wrong command.
const PERMISSION_ID_MIN: usize = 12;
/// How long `ask answer` waits to hear whether the session took the answer.
const CONFIRM: Duration = Duration::from_secs(2);

/// `ask answer --json`: one line saying how it went (schema 1, T1.4).
/// `result` is `answered`, `too-late` or `not-waiting`; `session` is the
/// session's name, or the start of its id.
fn print_result(result: &str, who: &str) {
    println!(
        "{}",
        json!({ "schema": JSON_SCHEMA, "result": result, "session": who })
    );
}

fn answer(store: &Store, id: &str, reply: &Reply, by: By, as_json: bool) -> Result<()> {
    let now = now_ms();
    let ask = match store.find_open(id, now) {
        Ok(ask) => ask,
        Err(e) => {
            // Answered or closed already: as late as an answer can be.
            if as_json && matches!(e, AnswerError::Closed | AnswerError::AlreadyAnswered) {
                if let Some(ask) = store.get(id) {
                    print_result("too-late", &who_of(&ask));
                }
            }
            return Err(answer_error(&e, id));
        }
    };
    if ask.kind == Kind::Permission && id != ask.id && id.len() < PERMISSION_ID_MIN {
        bail!(tr!(
            format!(
                "to answer a permission request, give at least {PERMISSION_ID_MIN} characters of its id (or all of it: {})",
                ask.id
            ),
            format!(
                "回答权限请求时，id 至少写 {PERMISSION_ID_MIN} 位（或者写全：{}）",
                ask.id
            )
        ));
    }
    let answer = build(&ask, reply, by)?;
    if let Err(e) = store.answer(&ask.id, &answer) {
        if as_json && matches!(e, AnswerError::Closed | AnswerError::AlreadyAnswered) {
            print_result("too-late", &who_of(&ask));
        }
        return Err(answer_error(&e, id));
    }
    let who = who_of(&ask);
    let reason = outcome(store, &ask.id);
    if as_json {
        match reason {
            Some(Reason::Answered) => print_result("answered", &who),
            None => print_result("not-waiting", &who),
            Some(Reason::Timeout | Reason::AnsweredInSession | Reason::SessionEnded) => {
                print_result("too-late", &who)
            }
            Some(Reason::InvalidAnswer) => {}
        }
    }
    match reason {
        Some(Reason::Answered) => {
            if !as_json {
                println!("{}", done_line(&ask, &answer, &who));
            }
            Ok(())
        }
        None if as_json => Ok(()),
        None => {
            println!(
                "{}",
                tr!(
                    format!(
                        "Saved, but {who} isn't waiting on it right now (it may have been answered in its terminal). It's handed over if {who} asks again."
                    ),
                    format!("答案记下了，但 {who} 这会儿没在等（可能已经在终端里答了）。它再问时会交过去。")
                )
            );
            Ok(())
        }
        Some(Reason::InvalidAnswer) => bail!(tr!(
            "the session could not use that answer",
            "会话用不了这个答案"
        )),
        Some(Reason::Timeout) => bail!(tr!(
            format!("too late: {who} stopped waiting for an answer"),
            format!("晚了一步：{who} 已经不等外面的答案了")
        )),
        Some(Reason::AnsweredInSession | Reason::SessionEnded) => bail!(tr!(
            format!("too late: {who} already moved on"),
            format!("晚了一步：{who} 已经往下走了")
        )),
    }
}

fn who_of(ask: &Ask) -> String {
    ask.session_name
        .clone()
        .unwrap_or_else(|| ask.session_id.chars().take(8).collect())
}

/// Why question `id` closed, once the hook waiting on it has looked; `None`
/// when no hook picked the answer up within [`CONFIRM`].
fn outcome(store: &Store, id: &str) -> Option<Reason> {
    let deadline = Instant::now() + CONFIRM;
    loop {
        if let Some(closed) = store.closed(id) {
            return Some(closed.reason);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(ask::POLL);
    }
}

/// What was handed over, said back: for a permission, which session, tool
/// and command.
fn done_line(ask: &Ask, answer: &Answer, who: &str) -> String {
    let Some(tool) = ask.tool.as_ref().filter(|_| ask.kind == Kind::Permission) else {
        return tr!(
            format!("Answered. {who} carries on with it."),
            format!("答好了，{who} 会接着往下做。")
        );
    };
    let (name, preview) = (&tool.name, &tool.preview);
    match answer.behavior {
        Some(Behavior::Allow) => tr!(
            format!("Allowed. {who} runs {name}: {preview}"),
            format!("已同意，{who} 会运行 {name}：{preview}")
        ),
        _ => tr!(
            format!("Denied. {who} won't run {name}: {preview}"),
            format!("已拒绝，{who} 不会运行 {name}：{preview}")
        ),
    }
}

fn answer_error(e: &AnswerError, id: &str) -> anyhow::Error {
    anyhow!(match e {
        AnswerError::NotFound => tr!(
            format!("no question `{id}` (see `cc-moss ask list`)"),
            format!("没有 `{id}` 这道题（看看 `cc-moss ask list`）")
        ),
        AnswerError::Closed => tr!(
            format!("question {id} no longer needs an answer"),
            format!("{id} 这道题已经不用答了")
        ),
        AnswerError::AlreadyAnswered => tr!(
            format!("someone already answered question {id}"),
            format!("{id} 这道题已经有人答了")
        ),
        AnswerError::Invalid => tr!(
            format!("that answer doesn't fit question {id} (see `cc-moss ask list`)"),
            format!("这个答案对不上 {id} 这道题（看看 `cc-moss ask list`）")
        ),
        AnswerError::Io(err) => format!("{err}"),
    })
}

fn take(store: &Store, name: &str) -> Result<()> {
    if !ask::valid_taker_name(name) {
        bail!(tr!(
            format!("`{name}`: taker names use a-z, 0-9, - and _"),
            format!("`{name}`：名字只能用 a-z、0-9、- 和 _")
        ));
    }
    store.take(name)?;
    store.sweep(now_ms());
    Ok(())
}
