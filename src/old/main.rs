use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use reqwest::Client;
use serde::Deserialize; // Removed unused 'Serialize'
use serde_json::Value;
use std::fmt::Write;

// ==========================================
// CONFIGURATION
// ==========================================
const AUTH_URL: &str = "https://prod.personifyauth.com/connect/token";
const API_BASE: &str = "https://api-prod.psfye.com/api/v1/floorplan/event";
const DEFAULT_TENANT: &str = "a36b138d-f02c-458c-bc58-b55db7b7af42";
const DEFAULT_EVENT: &str = "192";
const DEFAULT_CLIENT_ID: &str = "a2z_events_private_client_freeman";

// ==========================================
// EMBEDDED HTML TEMPLATE
// ==========================================
// We use a raw string literal (r#...#) so we can paste HTML directly.
const HTML_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <title>A2Z to AutoCAD (Rust)</title>
    <style>
        :root { --fg: #323130; --bg: #FFFFFF; --border: #ccc; }
        body { font-family: 'Segoe UI', sans-serif; background: var(--bg); color: var(--fg); padding: 40px; display: flex; flex-direction: column; align-items: center; }
        .container { width: 100%; max-width: 900px; }
        h1 { border-bottom: 2px solid var(--fg); padding-bottom: 10px; }
        .card { border: 1px solid var(--border); padding: 20px; border-radius: 4px; margin-bottom: 20px; background: #fafafa; }
        .error-card { border-left: 5px solid red; background: #fff0f0; }
        .form-group { margin-bottom: 15px; }
        label { display: block; font-weight: bold; margin-bottom: 5px; }
        input { width: 100%; padding: 10px; border: 1px solid #aaa; box-sizing: border-box; }
        button { background: var(--fg); color: var(--bg); padding: 12px 24px; border: none; cursor: pointer; font-weight: bold; }
        button:hover { opacity: 0.9; }
        pre { background: #f0f0f0; padding: 15px; overflow-x: auto; border: 1px solid #ddd; }
    </style>
</head>
<body>
    <div class="container">
        <h1>A2Z Floorplan Exporter (Rust)</h1>
        <div class="card">
            <form method="POST" action="/fetch">
                <div class="form-group"><label>Client ID</label><input type="text" name="client_id" value="{{client_id}}" required></div>
                <div class="form-group"><label>Client Secret</label><input type="password" name="client_secret" placeholder="Enter Secret" required></div>
                <div class="form-group"><label>Tenant ID</label><input type="text" name="tenant_id" value="{{tenant_id}}" required></div>
                <div class="form-group"><label>Event ID</label><input type="text" name="event_id" value="{{event_id}}" required></div>
                <button type="submit">Authenticate & Fetch</button>
            </form>
        </div>
        {{error_block}}
        {{result_block}}
    </div>
</body>
</html>
"#;

// ==========================================
// DATA STRUCTURES
// ==========================================
#[derive(Deserialize)]
struct FetchForm {
    client_id: String,
    client_secret: String,
    tenant_id: String,
    event_id: String,
}

#[derive(Deserialize)]
struct DownloadForm {
    json_data: String,
}

#[derive(Deserialize)]
struct AuthResponse {
    access_token: String,
}

// ==========================================
// LOGIC: SCR CONVERSION
// ==========================================
fn json_to_scr(json_str: &str) -> String {
    let data: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return "; Error parsing JSON data".to_string(),
    };

    let mut scr = String::new();
    writeln!(&mut scr, ";; A2Z Floorplan Import Script").unwrap();
    writeln!(&mut scr, "OSMODE 0").unwrap();
    writeln!(&mut scr, "ZOOM E").unwrap();
    writeln!(&mut scr, "-LAYER M BOOTHS C 2 BOOTHS ").unwrap();
    writeln!(&mut scr, "-LAYER M BOOTH_TEXT C 7 BOOTH_TEXT ").unwrap();

    let empty_vec = Vec::new();
    let items = if data.is_array() {
        data.as_array().unwrap()
    } else {
        data.get("booths").or_else(|| data.get("items")).and_then(|v| v.as_array()).unwrap_or(&empty_vec)
    };

    for booth in items {
        let b_id = booth.get("boothNumber").or_else(|| booth.get("boothId")).and_then(|v| v.as_str()).unwrap_or("???");
        let get_f64 = |key: &str, default: f64| -> f64 {
            booth.get(key).and_then(|v| {
                if v.is_number() { v.as_f64() } else if v.is_string() { v.as_str().unwrap().parse::<f64>().ok() } else { None }
            }).unwrap_or(default)
        };

        let x = get_f64("x", 0.0);
        let y = get_f64("y", 0.0);
        let w = get_f64("width", 10.0);
        let d = get_f64("depth", 10.0);
        let x2 = x + w;
        let y2 = y - d;
        let cx = x + (w / 2.0);
        let cy = y - (d / 2.0);
        let h = if w < d { w / 4.0 } else { d / 4.0 };

        writeln!(&mut scr, "CLAYER BOOTHS").unwrap();
        writeln!(&mut scr, "PLINE {:.2},{:.2} {:.2},{:.2} {:.2},{:.2} {:.2},{:.2} C", x, y, x2, y, x2, y2, x, y2).unwrap();
        writeln!(&mut scr, "CLAYER BOOTH_TEXT").unwrap();
        writeln!(&mut scr, "TEXT M {:.2},{:.2} {:.2} 0 {}", cx, cy, h, b_id).unwrap();
    }
    writeln!(&mut scr, "ZOOM E").unwrap();
    writeln!(&mut scr, "QSAVE").unwrap();
    scr
}

// ==========================================
// ROUTES
// ==========================================
async fn index() -> impl Responder {
    let rendered = HTML_TEMPLATE
        .replace("{{client_id}}", DEFAULT_CLIENT_ID)
        .replace("{{tenant_id}}", DEFAULT_TENANT)
        .replace("{{event_id}}", DEFAULT_EVENT)
        .replace("{{result_block}}", "")
        .replace("{{error_block}}", "");
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn fetch(form: web::Form<FetchForm>) -> impl Responder {
    let client = Client::new();
    let auth_params = [
        ("client_id", &form.client_id),
        ("client_secret", &form.client_secret),
        ("grant_type", &String::from("client_credentials")),
    ];

    let auth_res = client.post(AUTH_URL).form(&auth_params).send().await;
    let token = match auth_res {
        Ok(res) => {
            if !res.status().is_success() { return render_error(&format!("Auth Failed: {}", res.status()), &form); }
            match res.json::<AuthResponse>().await { Ok(body) => body.access_token, Err(_) => return render_error("Bad Auth Response", &form), }
        },
        Err(e) => return render_error(&format!("Conn Error: {}", e), &form),
    };

    let api_url = format!("{}/{}", API_BASE, form.event_id);
    let fp_res = client.get(&api_url)
        .header("Authorization", format!("Bearer {}", token))
        .header("X-Tenant-Id", &form.tenant_id)
        .header("Accept", "application/json")
        .send().await;

    let json_data = match fp_res {
        Ok(res) => {
            if !res.status().is_success() { return render_error(&format!("API Failed: {}", res.status()), &form); }
            match res.text().await { Ok(txt) => txt, Err(_) => return render_error("Read Error", &form), }
        },
        Err(e) => return render_error(&format!("Conn Error: {}", e), &form),
    };

    let pretty_json = serde_json::from_str::<Value>(&json_data)
        .map(|v| serde_json::to_string_pretty(&v).unwrap()).unwrap_or_else(|_| json_data.clone());

    let result_html = format!(
        r#"<div class="card"><h2>Success</h2><div class="actions"><form action="/download" method="POST"><input type="hidden" name="json_data" value="{}"><button type="submit">Download .SCR</button></form></div><h3>Raw JSON:</h3><pre>{}</pre></div>"#,
        html_escape(&json_data), pretty_json
    );

    let rendered = HTML_TEMPLATE
        .replace("{{client_id}}", &form.client_id)
        .replace("{{tenant_id}}", &form.tenant_id)
        .replace("{{event_id}}", &form.event_id)
        .replace("{{result_block}}", &result_html)
        .replace("{{error_block}}", "");

    HttpResponse::Ok().content_type("text/html").body(rendered)
}

async fn download(form: web::Form<DownloadForm>) -> impl Responder {
    let scr = json_to_scr(&form.json_data);
    HttpResponse::Ok().content_type("application/octet-stream").append_header(("Content-Disposition", "attachment; filename=\"floorplan.scr\"")).body(scr)
}

fn render_error(msg: &str, form: &FetchForm) -> HttpResponse {
    let err_html = format!(r#"<div class="card error-card"><h3>Error</h3><p>{}</p></div>"#, msg);
    let rendered = HTML_TEMPLATE
        .replace("{{client_id}}", &form.client_id)
        .replace("{{tenant_id}}", &form.tenant_id)
        .replace("{{event_id}}", &form.event_id)
        .replace("{{result_block}}", "")
        .replace("{{error_block}}", &err_html);
    HttpResponse::Ok().content_type("text/html").body(rendered)
}

fn html_escape(input: &str) -> String { input.replace("\"", "&quot;").replace("<", "&lt;").replace(">", "&gt;") }

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    println!("Server running at http://127.0.0.1:8080");
    
    HttpServer::new(|| {
        App::new()
            // FIX: Increase the form limit from 16KB to 10MB
            .app_data(web::FormConfig::default().limit(10 * 1024 * 1024)) 
            .route("/", web::get().to(index))
            .route("/fetch", web::post().to(fetch))
            .route("/download", web::post().to(download))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
