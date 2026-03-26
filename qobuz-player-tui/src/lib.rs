use std::sync::Arc;

use app::{App, get_current_state};
use favorites::FavoritesState;
use qobuz_player_controls::{
    AppResult, ExitSender, PositionReceiver, StatusReceiver, TracklistReceiver, client::Client,
    controls::Controls, error::Error, notification::NotificationBroadcast,
};
use tokio::try_join;
use queue::QueueState;
use ratatui::{prelude::*, widgets::*};
use ui::center;

mod app;
mod discover;
mod favorites;
mod genres;
mod now_playing;
mod popup;
mod queue;
mod search;
mod sub_tab;
mod ui;
mod widgets;

#[allow(clippy::too_many_arguments)]
pub async fn init(
    client: Arc<Client>,
    broadcast: Arc<NotificationBroadcast>,
    controls: Controls,
    position_receiver: PositionReceiver,
    tracklist_receiver: TracklistReceiver,
    status_receiver: StatusReceiver,
    exit_sender: ExitSender,
    disable_tui_album_cover: bool,
) -> AppResult<()> {
    let mut terminal = ratatui::init();

    draw_loading_screen(&mut terminal);

    let tracklist_value = tracklist_receiver.borrow().clone();
    let status_value = *status_receiver.borrow();
    let queue = tracklist_value.queue().into_iter().cloned().collect();

    // Load TUI state in parallel for faster startup
    let (now_playing, api_results) = tokio::join!(
        get_current_state(tracklist_value, status_value),
        async {
            try_join!(
                FavoritesState::new(&client),
                discover::DiscoverState::new(&client),
                genres::GenresState::new(&client),
            )
        }
    );
    let (favorites, discover, genres) = api_results?;

    let mut app = App {
        broadcast,
        notifications: Default::default(),
        controls,
        now_playing,
        full_screen: false,
        position: position_receiver,
        tracklist: tracklist_receiver,
        status: status_receiver,
        current_screen: Default::default(),
        exit: Default::default(),
        should_draw: true,
        app_state: Default::default(),
        disable_tui_album_cover,
        favorites,
        search: Default::default(),
        queue: QueueState::new(queue),
        discover,
        genres,
        client,
    };

    _ = app.run(&mut terminal).await;
    ratatui::restore();
    match exit_sender.send(true) {
        Ok(_) => Ok(()),
        Err(_) => Err(Error::Notification),
    }
}

fn draw_loading_screen<B: Backend>(terminal: &mut Terminal<B>) {
    let ascii_art = r#"
             _                     _                       
  __ _  ___ | |__  _   _ _____ __ | | __ _ _   _  ___ _ __ 
 / _` |/ _ \| '_ \| | | |_  / '_ \| |/ _` | | | |/ _ \ '__|
| (_| | (_) | |_) | |_| |/ /| |_) | | (_| | |_| |  __/ |   
 \__, |\___/|_.__/ \__,_/___| .__/|_|\__,_|\__, |\___|_|   
    |_|                     |_|            |___/           
"#;

    terminal
        .draw(|f| {
            let area = center(f.area(), Constraint::Length(64), Constraint::Length(7));
            let paragraph = Paragraph::new(ascii_art)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: false });
            f.render_widget(paragraph, area);
        })
        .expect("infallible");
}
