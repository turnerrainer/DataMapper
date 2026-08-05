import express from "express";
import path from "path";
import fs from "fs";
import { fileURLToPath } from "url";
import { engine } from "express-handlebars";
import * as helpers from "./lib/helpers.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const app = express();
const PORT = process.env.PORT || 3000;

app.use(express.json({ limit: "2mb" }));
app.use(express.urlencoded({ extended: true }));

app.engine("hbs", engine({ extname: ".hbs", helpers, layoutsDir: path.join(__dirname, "views", "layouts") }));
app.set("view engine", "hbs");

app.set("views", [path.join(__dirname, "views"), path.join(__dirname, "DSL")]);

const sanitize = (s) => path.posix.normalize(String(s || "")).replace(/^(\.{2}(\/|\\|$))+/, "").replace(/^\/+/, "");
const stripHbs = (name) => name.endsWith(".hbs") ? name.slice(0, -4) : name;

function wantsJson(req) {
  // original flag still supported
  const explicit = (req.get("type") || "").toLowerCase() === "json";
  // also honor the Accept header (json > html)
  const accepts = req.accepts(["json", "html"]);
  return explicit || accepts === "json";
}

function renderFirst(req, res, logicalNames) {
  const roots = Array.isArray(app.get("views")) ? app.get("views") : [app.get("views")];
  const found = logicalNames.find(name => roots.some(root => fs.existsSync(path.join(root, name + ".hbs"))));
  if (!found) {
    return res.status(404).json({ error: "TemplateNotFound", tried: logicalNames });
  }

  res.render(found, { layout: false, ...req.body }, (err, html) => {
    if (err) return res.status(500).json({ error: "TemplateRenderError", message: err.message, view: found });

    // If the client prefers JSON (Accept or custom header), or if the output is valid JSON, send JSON.
    if (wantsJson(req)) {
      try { return res.status(200).json(JSON.parse(html)); }
      catch { return res.status(200).type("application/json").send(html); } // send raw but with JSON MIME
    }

    // If the output is actually JSON, be helpful and send JSON anyway
    try {
      const obj = JSON.parse(html);
      return res.status(200).json(obj);
    } catch (_) {
      // not JSON → send as text/HTML
      return res.status(200).send(html);
    }
  });
}

app.post("/:project/*", (req, res) => {
  if (req.params.project === "healthz") return res.status(405).json({ error: "MethodNotAllowed" });
  const project = sanitize(req.params.project);
  const rest = stripHbs(sanitize(req.params[0] || ""));
  const candidates = [
    path.posix.join(project, rest),
    path.posix.join(project, "hbs", rest)
  ];
  renderFirst(req, res, candidates);
});

app.get("/healthz", (req, res) => res.status(200).json({ service: "DataMapper", ok: true, ts: new Date().toISOString() }));

app.listen(PORT, () => console.log(`DataMapper listening on :${PORT}`));
