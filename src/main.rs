use actix::prelude::*;
use actix_web::{web, App, HttpServer, HttpResponse, Error, HttpRequest};
use actix_web_actors::ws;
use actix_cors::Cors;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

#[derive(Serialize, Deserialize, Clone)]
struct Room {
    id: Uuid,
    name: String,
    creator: String,
    users: HashSet<String>,
}

#[derive(Default)]
struct AppState {
    users: Mutex<HashMap<String, String>>,               // username -> password
    rooms: Mutex<HashMap<Uuid, Room>>,                  // room_id -> Room
    connections: Mutex<HashMap<Uuid, Vec<Addr<WebSocketSession>>>>, // room_id -> WebSocket connections
}

#[derive(Debug, Deserialize)]
struct RegisterRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct CreateRoomRequest {
    name: String,
    creator: String,
}

#[derive(Deserialize)]
struct AddUserRequest {
    room_id: Uuid,
    username: String,
}

#[derive(Deserialize, Message)]
#[rtype(result = "()")]
struct ChatMessage {
    room_id: Uuid,
    username: String,
    message: String,
}

// WebSocket Session
struct WebSocketSession {
    room_id: Uuid,
    username: String,
    app_state: Arc<AppState>,
}

impl Actor for WebSocketSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let mut connections = self.app_state.connections.lock().unwrap();
        connections
            .entry(self.room_id)
            .or_insert_with(Vec::new)
            .push(ctx.address());
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        let mut connections = self.app_state.connections.lock().unwrap();
        if let Some(users) = connections.get_mut(&self.room_id) {
            users.retain(|addr| addr != &ctx.address());
        }
    }
}

impl Handler<ChatMessage> for WebSocketSession {
    type Result = ();

    fn handle(&mut self, msg: ChatMessage, ctx: &mut Self::Context) {
        if msg.room_id == self.room_id {
            ctx.text(msg.message);
        }
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WebSocketSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        if let Ok(ws::Message::Text(text)) = msg {
            if let Ok(text_string) = String::from_utf8(text.as_bytes().to_vec()) {
                let connections = self.app_state.connections.lock().unwrap();
                if let Some(users) = connections.get(&self.room_id) {
                    for user in users {
                        user.do_send(ChatMessage {
                            room_id: self.room_id,
                            username: self.username.clone(),
                            message: text_string.clone(),
                        });
                    }
                }
            } else {
                ctx.text("Invalid UTF-8 data received.");
            }
        }
    }
}


// WebSocket handler
async fn websocket_handler(
    req: HttpRequest,
    stream: web::Payload,
    data: web::Data<Arc<AppState>>,
) -> Result<HttpResponse, actix_web::Error> {
    let query_string = req.query_string();
    let query_params: HashMap<String, String> = serde_urlencoded::from_str(query_string)
        .map_err(|_| actix_web::error::ErrorBadRequest("Invalid query string"))?;

    let room_id = query_params
        .get("roomId")
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or_else(|| actix_web::error::ErrorBadRequest("Missing or invalid roomId"))?;

    let username = query_params
        .get("username")
        .cloned()
        .unwrap_or_else(|| "guest".to_string());

    ws::start(
        WebSocketSession {
            room_id,
            username,
            app_state: data.get_ref().clone(),
        },
        &req,
        stream,
    )
}

// REST API Handlers
async fn register(data: web::Data<Arc<AppState>>, req: web::Json<RegisterRequest>) -> HttpResponse {
    log::info!("Incoming register request: {:?}", req);

    let mut users = data.users.lock().unwrap_or_else(|e| {
        log::error!("Failed to lock users: {:?}", e);
        panic!("Mutex poisoned");
    });

    if users.contains_key(&req.username) {
        log::warn!("User already exists: {}", req.username);
        return HttpResponse::Conflict().body("User already exists");
    }

    users.insert(req.username.clone(), req.password.clone());
    log::info!("User registered successfully: {}", req.username);

    HttpResponse::Ok().body("User registered successfully")
}

async fn login(data: web::Data<Arc<AppState>>, req: web::Json<LoginRequest>) -> HttpResponse {
    let users = data.users.lock().unwrap();
    if let Some(password) = users.get(&req.username) {
        if password == &req.password {
            return HttpResponse::Ok().body("Login successful");
        }
    }
    HttpResponse::Unauthorized().body("Invalid username or password")
}

async fn create_room(data: web::Data<Arc<AppState>>, req: web::Json<CreateRoomRequest>) -> HttpResponse {
    let mut rooms = data.rooms.lock().unwrap();
    let room = Room {
        id: Uuid::new_v4(),
        name: req.name.clone(),
        creator: req.creator.clone(),
        users: HashSet::new(),
    };
    rooms.insert(room.id, room.clone());
    HttpResponse::Ok().json(room)
}

async fn add_user(data: web::Data<Arc<AppState>>, req: web::Json<AddUserRequest>) -> HttpResponse {
    let mut rooms = data.rooms.lock().unwrap();
    if let Some(room) = rooms.get_mut(&req.room_id) {
        room.users.insert(req.username.clone());
        return HttpResponse::Ok().json(room.clone());
    }
    HttpResponse::NotFound().body("Room not found")
}

async fn list_rooms(data: web::Data<Arc<AppState>>) -> HttpResponse {
    let rooms = data.rooms.lock().unwrap();
    let room_list: Vec<_> = rooms.values().cloned().collect();
    HttpResponse::Ok().json(room_list)
}


use env_logger;
use log::info;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    std::env::set_var("RUST_LOG", "debug");
    env_logger::init();
    info!("Starting server...");

    let app_state = Arc::new(AppState::default());

    HttpServer::new(move || {
        App::new()
            .wrap(
                Cors::default()
                    .allow_any_origin()
                    .allow_any_header()
                    .allow_any_method(),
            )
            .app_data(web::Data::new(app_state.clone()))
            .route("/register", web::post().to(register))
            .route("/login", web::post().to(login))
            .route("/create_room", web::post().to(create_room))
            .route("/add_user", web::post().to(add_user))
            .route("/list_rooms", web::get().to(list_rooms))
            .route("/ws/", web::get().to(websocket_handler))
    })
        .bind("127.0.0.1:8080")?
        .run()
        .await
}

