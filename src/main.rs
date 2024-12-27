use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use actix_web::{get, post};
use actix_files as fs;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use actix_cors::Cors;
use uuid::Uuid;

mod chat_websocket {
    use actix::{Actor, StreamHandler};
    use actix_web::{HttpRequest, HttpResponse, Error};
    use actix_web_actors::ws;

    pub struct ChatSession;

    impl Actor for ChatSession {
        type Context = ws::WebsocketContext<Self>;
    }

    impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for ChatSession {
        fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
            if let Ok(ws::Message::Text(text)) = msg {
                ctx.text(text);
            }
        }
    }

    pub async fn start_chat(req: HttpRequest, stream: actix_web::web::Payload) -> Result<HttpResponse, Error> {
        ws::start(ChatSession, &req, stream)
    }
}

use chat_websocket::start_chat;

#[derive(Debug, Serialize, Deserialize)]
struct User {
    username: String,
    password: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    sender: String,
    recipient: String,
    content: String,
}

struct AppState {
    users: Mutex<HashMap<String, String>>, // username -> password
    messages: Mutex<HashMap<String, Vec<Message>>>, // recipient -> messages
}

#[post("/register")]
async fn register(user: web::Json<User>, data: web::Data<AppState>) -> impl Responder {
    if user.username.trim().is_empty() || user.password.trim().is_empty() {
        return HttpResponse::BadRequest().body("Username and password cannot be empty");
    }

    let mut users = data.users.lock().unwrap();

    if users.contains_key(&user.username) {
        return HttpResponse::BadRequest().body("User already exists");
    }

    users.insert(user.username.clone(), user.password.clone());
    HttpResponse::Ok().body("User registered")
}

#[post("/login")]
async fn login(user: web::Json<User>, data: web::Data<AppState>) -> impl Responder {
    if user.username.trim().is_empty() || user.password.trim().is_empty() {
        return HttpResponse::BadRequest().body("Username and password cannot be empty");
    }

    let users = data.users.lock().unwrap();

    match users.get(&user.username) {
        Some(password) if password == &user.password => HttpResponse::Ok().body("Login successful"),
        _ => HttpResponse::Unauthorized().body("Invalid credentials"),
    }
}

#[get("/history/{username}")]
async fn get_history(username: web::Path<String>, data: web::Data<AppState>) -> impl Responder {
    let messages = data.messages.lock().unwrap();

    if let Some(user_messages) = messages.get(&username.into_inner()) {
        HttpResponse::Ok().json(user_messages)
    } else {
        HttpResponse::Ok().json(Vec::<Message>::new())
    }
}

#[derive(Deserialize)]
struct MessageData {
    sender: String,
    recipient: String,
    content: String,
}

#[post("/send_message")]
async fn send_message(msg: web::Json<MessageData>, data: web::Data<AppState>) -> impl Responder {
    if msg.sender.trim().is_empty() || msg.recipient.trim().is_empty() || msg.content.trim().is_empty() {
        return HttpResponse::BadRequest().body("Sender, recipient, and content cannot be empty");
    }

    let mut messages = data.messages.lock().unwrap();
    let recipient_messages = messages.entry(msg.recipient.clone()).or_insert_with(Vec::new);
    recipient_messages.push(Message {
        sender: msg.sender.clone(),
        recipient: msg.recipient.clone(),
        content: msg.content.clone(),
    });

    HttpResponse::Ok().body("Message sent")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let app_state = web::Data::new(AppState {
        users: Mutex::new(HashMap::new()),
        messages: Mutex::new(HashMap::new()),
    });

    HttpServer::new(move || {
        App::new()
            .wrap(Cors::default()
                .allow_any_origin()
                .allow_any_method()
                .allow_any_header())
            .app_data(app_state.clone())
            .service(register)
            .service(login)
            .service(get_history)
            .service(send_message)
            .route("/ws/", web::get().to(start_chat))
            .service(fs::Files::new("/", "./static").index_file("index.html"))
    })
        .bind("127.0.0.1:8080")?
        .run()
        .await
}
