use blackletter::App;

fn main() -> iced::Result {
    env_logger::init();
    iced::daemon(App::new, App::update, App::view)
        .subscription(App::subscription)
        .title("Blackletter")
        .theme(App::theme)
        .run()
}
