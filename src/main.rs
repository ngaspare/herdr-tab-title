use herdr_plugin::{
    App, Context, OneShotRuntime, TabClosed, TabCreated, TabFocused, TabMoved, TabRenamed,
    WorkspaceClosed, WorkspaceCreated, WorkspaceFocused, WorkspaceRenamed,
};
use herdr_tab_title::config::Config;
use herdr_tab_title::formatter::Formatter;
use herdr_tab_title::refresh_lock::{REFRESH_LOCK_DELAY, RefreshLockError, run_cross_process};
use herdr_tab_title::rename;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("herdr-tab-title: {error}");
    }
}

async fn run() -> Result<(), String> {
    App::builder()
        .runtime(OneShotRuntime::new())
        .with_config::<Config>()
        .build()
        .map_err(|error| error.to_string())?
        .setup(|ctx: Context<(), Config>| async move {
            if ctx.env().plugin_event_json.is_none() {
                refresh_with_lock(ctx, None).await.map_err(|error| {
                    let error = std::io::Error::new(std::io::ErrorKind::Other, error);
                    herdr_plugin::SetupError::from(error)
                })?;
            }

            Ok(())
        })
        .on_event::<TabCreated>(|ctx: Context<(), Config>, event: TabCreated| async move {
            if let Err(error) = refresh_with_lock(ctx, Some(event.tab.tab_id)).await {
                eprintln!("herdr-tab-title: {error}");
            }
        })
        .on_event::<TabMoved>(refresh_all)
        .on_event::<TabRenamed>(refresh_all)
        .on_event::<TabClosed>(refresh_all)
        .on_event::<TabFocused>(refresh_all)
        .on_event::<WorkspaceCreated>(refresh_all)
        .on_event::<WorkspaceFocused>(refresh_all)
        .on_event::<WorkspaceRenamed>(refresh_all)
        .on_event::<WorkspaceClosed>(refresh_all)
        .run()
        .await
        .map_err(|error| error.to_string())
}

async fn refresh_all<E>(ctx: Context<(), Config>, _event: E)
where
    E: Send + 'static,
{
    if let Err(error) = refresh_with_lock(ctx, None).await {
        eprintln!("herdr-tab-title: {error}");
    }
}

async fn refresh_with_lock(
    ctx: Context<(), Config>,
    created_tab_id: Option<String>,
) -> Result<(), String> {
    let state_dir = ctx.env().plugin_state_dir.clone();
    let client = ctx.client().clone();
    let formatter = Formatter::parse(&ctx.config().format).map_err(|error| error.to_string())?;

    tokio::task::spawn_blocking(move || {
        run_cross_process(state_dir.as_deref(), REFRESH_LOCK_DELAY, || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| RefreshLockError::Action(error.to_string()))?;

            runtime
                .block_on(async {
                    if let Some(tab_id) = created_tab_id.as_deref() {
                        rename::refresh_created_tab(&client, &formatter, tab_id).await?;
                    } else {
                        rename::refresh(&client, &formatter).await?;
                    }

                    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                })
                .map_err(|error| RefreshLockError::Action(error.to_string()))
        })
    })
    .await
    .map_err(|error| error.to_string())?
    .map(|_| ())
    .map_err(|error| error.to_string())
}
