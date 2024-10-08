use iced::widget::{button, column, text, text_input, Space};
use iced::{event, Alignment, Element, Event, Length, Task as Command, Theme};

use iced_sessionlock::actions::UnLockAction;
use iced_sessionlock::settings::Settings;
use iced_sessionlock::MultiApplication;

pub fn main() -> Result<(), iced_sessionlock::Error> {
    Counter::run(Settings::default())
}

struct Counter {
    value: i32,
    text: String,
}

#[derive(Debug, Clone)]
enum Message {
    IncrementPressed,
    DecrementPressed,
    TextInput(String),
    IcedEvent(Event),
    UnLock,
}

impl TryInto<UnLockAction> for Message {
    type Error = Self;
    fn try_into(self) -> Result<UnLockAction, Self::Error> {
        if let Self::UnLock = self {
            return Ok(UnLockAction);
        }
        Err(self)
    }
}

impl MultiApplication for Counter {
    type Message = Message;
    type Flags = ();
    type Theme = Theme;
    type Executor = iced::executor::Default;

    fn new(_flags: ()) -> (Self, Command<Message>) {
        (
            Self {
                value: 0,
                text: "lock".to_string(),
            },
            Command::none(),
        )
    }

    fn namespace(&self) -> String {
        String::from("Counter - Iced")
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        event::listen().map(Message::IcedEvent)
    }

    fn update(&mut self, message: Message) -> Command<Message> {
        match message {
            Message::IcedEvent(_event) => {
                Command::none()
            }
            Message::IncrementPressed => {
                self.value += 1;
                Command::none()
            }
            Message::DecrementPressed => {
                self.value -= 1;
                Command::none()
            }
            Message::TextInput(text) => {
                self.text = text;
                Command::none()
            }
            Message::UnLock => Command::done(message),
        }
    }

    fn view(&self, _id: iced::window::Id) -> Element<Message> {
        column![
            Space::with_height(Length::Fill),
            button("Increment").on_press(Message::IncrementPressed),
            button("UnLock").on_press(Message::UnLock),
            text(self.value).size(50),
            text_input("hello", &self.text)
                .on_input(Message::TextInput)
                .padding(10),
            button("Decrement").on_press(Message::DecrementPressed),
            Space::with_height(Length::Fill),
        ]
        .padding(20)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
    }
}
