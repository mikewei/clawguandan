use std::sync::{Arc, Mutex};

use serde_json::Value;

use super::traits::{
    ObserverGameOverContext, ObserverGameStartContext, ObserverHandOverContext,
    ObserverHandStartContext, ObserverPolicy,
};

#[derive(Debug, Default)]
pub struct DefaultObserverPolicy {
    plugin_id: Mutex<Option<String>>,
    last_narration_raw: Mutex<Option<String>>,
}

impl ObserverPolicy for DefaultObserverPolicy {
    fn on_transition(&self, transition: &Value, verbosity: u8) -> Result<(), String> {
        let tag = current_plugin_tag(&self.plugin_id)?;
        if let Some(raw) = last_narration_from_nextstate_json(transition) {
            let mut g = self.last_narration_raw.lock().map_err(|e| e.to_string())?;
            let changed = g.as_ref().map(|s| s.as_str()) != Some(raw.as_str());
            if changed {
                let disp = narration_display_en(&raw);
                if !disp.is_empty() {
                    println!("[{tag}][I][narration] text={disp}");
                }
                *g = Some(raw);
            }
        }
        if verbosity >= 3 {
            println!("[{tag}][T][transition] {}", transition);
        }
        Ok(())
    }

    fn on_game_start(&self, ctx: &ObserverGameStartContext) -> Result<(), String> {
        let mut tag = self.plugin_id.lock().map_err(|e| e.to_string())?;
        *tag = Some(ctx.plugin_id.clone());
        let hands_display = ctx
            .hands_target
            .map(|n| n.to_string())
            .unwrap_or_else(|| "until-game-end".to_string());
        println!(
            "[{}][I][game:start] table={} seq={} occupied={} vacancy={} join={} target_hands={}",
            ctx.plugin_id,
            ctx.table_id,
            ctx.transition_seq,
            ctx.occupied,
            ctx.vacancy,
            ctx.join_bots,
            hands_display
        );
        if ctx.verbosity >= 1 {
            println!(
                "[{}][D][observer:session] name={}",
                ctx.plugin_id, ctx.observer_name
            );
        }
        Ok(())
    }

    fn on_hand_start(&self, ctx: &ObserverHandStartContext) -> Result<(), String> {
        if ctx.verbosity >= 1 {
            println!(
                "[{}][I][hand:start] table={} hand={} seq={} type={}",
                ctx.plugin_id, ctx.table_id, ctx.hand_index, ctx.transition_seq, ctx.transition_type
            );
        }
        Ok(())
    }

    fn on_hand_over(&self, ctx: &ObserverHandOverContext) -> Result<(), String> {
        println!(
            "[{}][I][hand:over] table={} hand={} seq={} type={}",
            ctx.plugin_id, ctx.table_id, ctx.hand_index, ctx.transition_seq, ctx.transition_type
        );
        Ok(())
    }

    fn on_game_over(&self, ctx: &ObserverGameOverContext) -> Result<(), String> {
        println!(
            "[{}][I][game:over] table={} hands_done={} seq={} type={}",
            ctx.plugin_id, ctx.table_id, ctx.hands_done, ctx.transition_seq, ctx.transition_type
        );
        Ok(())
    }
}

pub fn default_observer() -> Arc<dyn ObserverPolicy> {
    Arc::new(DefaultObserverPolicy::default()) as Arc<dyn ObserverPolicy>
}

fn current_plugin_tag(cell: &Mutex<Option<String>>) -> Result<String, String> {
    let g = cell.lock().map_err(|e| e.to_string())?;
    Ok(g.clone().unwrap_or_else(|| "observer".to_string()))
}

fn last_narration_from_nextstate_json(v: &Value) -> Option<String> {
    let ops = v.get("delta")?.get("ops")?.as_array()?;
    let mut out: Option<String> = None;
    for op in ops {
        if op.get("op").and_then(|x| x.as_str()) == Some("replace")
            && op.get("path").and_then(|x| x.as_str()) == Some("/narration")
            && let Some(val) = op.get("value")
        {
            out = Some(match val {
                Value::String(s) => s.clone(),
                _ => val.to_string(),
            });
        }
    }
    out
}

fn narration_display_en(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    if let Ok(v) = serde_json::from_str::<Value>(t)
        && let Some(en) = v.get("en").and_then(|x| x.as_str())
    {
        return en.trim().to_string();
    }
    t.to_string()
}
