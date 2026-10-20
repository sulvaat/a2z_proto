use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use reqwest::Client;
use serde::Deserialize;
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
// THEMED HTML TEMPLATE (Freeman Brand)
// ==========================================
const HTML_TEMPLATE: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <title>Freeman Floorplan Exporter</title>
    <style>
        :root {
            /* Freeman Brand Colors */
            --freeman-blue: #00558C;
            --text-dark: #333333;
            --bg-white: #FFFFFF;
            --bg-light: #F4F4F4;
            --border-color: #E0E0E0;
        }
        body {
            font-family: 'Segoe UI', Roboto, Helvetica, Arial, sans-serif;
            background-color: var(--bg-white);
            color: var(--text-dark);
            margin: 0;
            padding: 40px;
            display: flex;
            flex-direction: column;
            align-items: center;
        }
        .container {
            width: 100%;
            max-width: 800px;
        }
        h1 {
            color: var(--freeman-blue);
            border-bottom: 2px solid var(--freeman-blue);
            padding-bottom: 15px;
            margin-bottom: 30px;
            font-weight: 600;
            letter-spacing: 0.5px;
        }
        .card {
            background: var(--bg-white);
            border: 1px solid var(--border-color);
            padding: 30px;
            border-radius: 4px;
            margin-bottom: 25px;
            box-shadow: 0 4px 12px rgba(0,0,0,0.08);
        }
        .error-card {
            border-left: 5px solid #D32F2F;
            background: #FFEBEE;
        }
        .form-group {
            margin-bottom: 20px;
        }
        label {
            display: block;
            font-weight: 600;
            margin-bottom: 8px;
            color: var(--freeman-blue);
            font-size: 0.9rem;
            text-transform: uppercase;
        }
        input {
            width: 100%;
            padding: 12px;
            border: 1px solid #CCC;
            background: #FAFAFA;
            box-sizing: border-box;
            font-size: 1rem;
            border-radius: 2px;
            transition: border 0.2s;
        }
        input:focus {
            border-color: var(--freeman-blue);
            outline: none;
            background: #FFF;
        }
        button {
            background-color: var(--freeman-blue);
            color: white;
            padding: 14px 28px;
            border: none;
            cursor: pointer;
            font-size: 16px;
            font-weight: 600;
            border-radius: 2px;
            transition: background-color 0.2s;
            text-transform: uppercase;
            letter-spacing: 1px;
            display: block;
            width: 100%;
        }
        button:hover {
            background-color: #003366;
        }
        pre {
            background: var(--bg-light);
            padding: 15px;
            overflow-x: auto;
            border: 1px solid var(--border-color);
            color: #444;
            font-size: 0.85rem;
        }
        .actions {
            margin-top: 20px;
            padding-top: 20px;
            border-top: 1px solid var(--border-color);
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>Freeman Floorplan Exporter</h1>
        
        <div class="card">
            <form method="POST" action="/fetch">
                <div class="form-group">
                    <label>Client ID</label>
                    <input type="text" name="client_id" value="{{client_id}}" required>
                </div>
                <div class="form-group">
                    <label>Client Secret</label>
                    <input type="password" name="client_secret" placeholder="Enter Client Secret" required>
                </div>
                <div class="form-group">
                    <label>Tenant ID</label>
                    <input type="text" name="tenant_id" value="{{tenant_id}}" required>
                </div>
                <div class="form-group">
                    <label>Event ID</label>
                    <input type="text" name="event_id" value="{{event_id}}" required>
                </div>
                <button type="submit">Authenticate & Fetch Data</button>
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
    writeln!(&mut scr, ";; Freeman A2Z Floorplan Import Script").unwrap();
    writeln!(&mut scr, "OSMODE 0").unwrap();
    writeln!(&mut scr, "ZOOM E").unwrap();
    // Layer Setup
    writeln!(&mut scr, "-LAYER M BOOTHS C 2 BOOTHS ").unwrap();         // Yellow (2) for lines
    writeln!(&mut scr, "-LAYER M BOOTH_TEXT C 7 BOOTH_TEXT ").unwrap(); // White (7) for Booth #
    writeln!(&mut scr, "-LAYER M EXHIBITOR_NAMES C 3 EXHIBITOR_NAMES ").unwrap(); // Green (3) for Names

    let empty_vec = Vec::new();
    let items = if data.is_array() {
        data.as_array().unwrap()
    } else {
        data.get("booths").or_else(|| data.get("items")).and_then(|v| v.as_array()).unwrap_or(&empty_vec)
    };

    for booth in items {
        // 1. Get Booth Number
        let b_id = booth.get("boothNumber")
            .or_else(|| booth.get("boothId"))
            .and_then(|v| v.as_str())
            .unwrap_or("???");

        // 2. Get Exhibitor Name (Check nested object or flattened field)
        let exh_name = booth.get("exhibitor")
            .and_then(|e| e.get("name").or_else(|| e.get("companyName")))
            .or_else(|| booth.get("exhibitorName"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
            
        // Sanitize name for AutoCAD (remove newlines/semicolons)
        let safe_exh_name = exh_name.replace(";", "").replace("\n", " ").replace("\r", "");

        // 3. Get Geometry
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
        let h = if w < d { w / 4.0 } else { d / 4.0 }; // Base text height

        // Draw Box
        writeln!(&mut scr, "CLAYER BOOTHS").unwrap();
        writeln!(&mut scr, "PLINE {:.2},{:.2} {:.2},{:.2} {:.2},{:.2} {:.2},{:.2} C", x, y, x2, y, x2, y2, x, y2).unwrap();
        
        // Draw Booth Number (Centered)
        writeln!(&mut scr, "CLAYER BOOTH_TEXT").unwrap();
        writeln!(&mut scr, "TEXT M {:.2},{:.2} {:.2} 0 {}", cx, cy, h, b_id).unwrap();

        // Draw Exhibitor Name (Below Booth Number)
        // Only if name exists. Text size is slightly smaller (80% of booth num)
        if !safe_exh_name.is_empty() {
             writeln!(&mut scr, "CLAYER EXHIBITOR_NAMES").unwrap();
             // Offset Y by (h * 1.5) to place it under the booth number
             writeln!(&mut scr, "TEXT M {:.2},{:.2} {:.2} 0 {}", cx, cy - (h * 1.5), h * 0.8, safe_exh_name).unwrap();
        }
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

    // UPDATE: Added ?include=exhibitor query param
    let api_url = format!("{}/{}?include=exhibitor", API_BASE, form.event_id);
    
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
        r#"<div class="card"><h2>Floorplan Data Retrieved</h2><div class="actions"><form action="/download" method="POST"><input type="hidden" name="json_data" value="{}"><button type="submit">Download .SCR File</button></form></div><h3>Raw JSON Data:</h3><pre>{}</pre></div>"#,
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
            .app_data(web::FormConfig::default().limit(10 * 1024 * 1024))
            .route("/", web::get().to(index))
            .route("/fetch", web::post().to(fetch))
            .route("/download", web::post().to(download))
    })
    .bind(("127.0.0.1", 8080))?
    .run()
    .await
}
