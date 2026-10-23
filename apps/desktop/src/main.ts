import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import "./styles.css";

type MenuMode = "convert" | "actions";
type ModifierKey = "shift" | "control";
type ActionId = "copy-path" | "copy" | "move" | "reveal" | "terminal";

type AnalyzeResponse = {
  files: Array<{
    path: string;
    name: string;
    kind: "image" | "video" | "pdf" | "document" | "presentation" | "spreadsheet" | "unsupported";
    targets: string[];
  }>;
  commonTargets: string[];
};

type ConversionResponse = { outputs: string[] };
type TransferResponse = { outputs: string[] };
type ExternalSelection = { paths: string[] };
type Point = { x: number; y: number };
type DragIntegrationStatus = {
  backend: string;
  ready: boolean;
  convertShortcut: string;
  actionsShortcut: string;
  detail: string;
};
type ModifierSettings = {
  convertModifier: ModifierKey;
  actionsModifier: ModifierKey;
};

const runningInTauri = "__TAURI_INTERNALS__" in window;
const currentWindow = runningInTauri ? getCurrentWindow() : null;
const isOverlay = currentWindow?.label === "overlay"
  || new URLSearchParams(window.location.search).has("overlayPreview");

const actionBySector: Array<ActionId | null> = [
  "copy-path",
  "copy",
  "move",
  "reveal",
  "terminal",
  null,
];

const actionLabels: string[][] = [
  ["YOLU", "KOPYALA"],
  ["KOPYALA"],
  ["TAŞI"],
  ["KLASÖRDE", "GÖSTER"],
  ["TERMİNAL"],
  [],
];

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("Application root was not found");
document.body.classList.toggle("overlay-window", isOverlay);
document.documentElement.classList.toggle("overlay-root", isOverlay);

app.innerHTML = `
  <main class="shell">
    <header class="topbar">
      <a class="brand" href="#" aria-label="Grapefruit ana sayfa">
        <span class="brand-mark" aria-hidden="true">
          <span></span><span></span><span></span>
        </span>
        <span>Grapefruit</span>
      </a>
      <div class="topbar-actions">
        <button id="open-settings" class="settings-launch" type="button">Ayarlar</button>
        <button id="open-overlay" class="overlay-launch" type="button">Sistem menüsünü aç</button>
        <div class="privacy-pill"><span></span> Yerel işleme</div>
      </div>
    </header>

    <section class="intro" aria-labelledby="page-title">
      <p class="eyebrow">DOSYA DÖNÜŞTÜRÜCÜ VE HIZLI İŞLEMLER</p>
      <h1 id="page-title">Sürükle. Hedefi seç.<br /><em>Bırak ve devam et.</em></h1>
      <p class="lede">Görsel, video, PDF, belge ve sunumları dönüştür; günlük dosya işlemlerini tek bir dairesel menüden tamamla.</p>
      <p id="global-status" class="global-status">Dosya sürükleme entegrasyonu hazırlanıyor…</p>
    </section>

    <section id="drop-stage" class="drop-stage" aria-label="Dosya bırakma alanı">
      <div class="stage-grid" aria-hidden="true"></div>
      <div class="shortcut-row" aria-label="Menü kısayolları">
        <button id="convert-mode" class="shortcut active" type="button" aria-pressed="true">
          <kbd id="convert-key">⇧</kbd><span><strong>Dönüştür</strong><small id="convert-key-help">Shift ile seç</small></span>
        </button>
        <button id="actions-mode" class="shortcut" type="button" aria-pressed="false">
          <kbd id="actions-key">Ctrl</kbd><span><strong>Dosya işlemleri</strong><small id="actions-key-help">Ctrl ile seç</small></span>
        </button>
      </div>

      <div id="wheel-wrap" class="wheel-wrap">
        <svg id="wheel" class="wheel" viewBox="-210 -210 420 420" role="img" aria-label="Dönüştürme hedefleri">
          <g id="segments"></g>
          <circle class="wheel-center" cx="0" cy="0" r="72"></circle>
          <g class="center-idle">
            <path d="M -18 -8 L 0 -25 L 18 -8 M 0 -24 L 0 18 M -18 18 L 18 18" />
            <text x="0" y="48">DOSYA SÜRÜKLE</text>
          </g>
          <g class="center-files is-hidden">
            <text id="file-count" class="file-count" x="0" y="-4">1</text>
            <text class="file-label" x="0" y="21">ÖĞE</text>
          </g>
        </svg>
        <div id="selection-preview" class="selection-preview" aria-hidden="true">
          <img id="selection-thumbnail" class="selection-thumbnail" alt="" />
          <span id="selection-extension" class="selection-extension">DOCX</span>
          <span id="selection-name" class="selection-name"></span>
          <span id="selection-more" class="selection-more"></span>
        </div>
        <div class="wheel-glow" aria-hidden="true"></div>
      </div>

      <div class="status-panel" aria-live="polite">
        <span id="status-icon" class="status-icon">→</span>
        <div>
          <strong id="status-title">Belgelerini buraya sürükle</strong>
          <p id="status-detail">PDF diliminin üzerine bıraktığında dönüşüm otomatik başlar.</p>
        </div>
      </div>
    </section>

    <footer>
      <span>Dönüşüm kaynakları değiştirmez</span>
      <span>•</span>
      <span>Kopyalama ve taşıma mevcut hedeflerin üzerine yazmaz</span>
    </footer>
  </main>

  <dialog id="settings-dialog" class="settings-dialog" aria-labelledby="settings-title">
    <form id="settings-form" method="dialog">
      <div class="settings-heading">
        <div>
          <p class="eyebrow">HIZLI MENÜ</p>
          <h2 id="settings-title">Tuş ayarları</h2>
        </div>
        <button class="dialog-close" type="button" data-close-settings aria-label="Ayarları kapat">×</button>
      </div>
      <p class="settings-copy">Dosya sürüklerken çemberi açan tuşu seç. Diğer işlev otomatik olarak kalan tuşa atanır.</p>
      <label class="settings-field" for="convert-modifier">
        <span><strong>Dönüştürme menüsü</strong><small>Çemberi bu tuşla açar.</small></span>
        <select id="convert-modifier" name="convertModifier">
          <option value="shift">Shift</option>
          <option value="control">Ctrl</option>
        </select>
      </label>
      <div class="settings-field settings-readonly">
        <span><strong>Dosya işlemleri menüsü</strong><small>Dönüştürme tuşunun diğeridir.</small></span>
        <kbd id="actions-modifier-preview">Ctrl</kbd>
      </div>
      <p id="settings-feedback" class="settings-feedback" aria-live="polite"></p>
      <div class="settings-actions">
        <button class="secondary-button" type="button" data-close-settings>Vazgeç</button>
        <button class="primary-button" type="submit" value="default">Kaydet</button>
      </div>
    </form>
  </dialog>
`;

const stage = requiredElement<HTMLElement>("#drop-stage");
const wheel = requiredElement<SVGSVGElement>("#wheel");
const segmentGroup = requiredElement<SVGGElement>("#segments");
const centerIdle = requiredElement<SVGGElement>(".center-idle");
const centerFiles = requiredElement<SVGGElement>(".center-files");
const fileCount = requiredElement<SVGTextElement>("#file-count");
const statusIcon = requiredElement<HTMLElement>("#status-icon");
const statusTitle = requiredElement<HTMLElement>("#status-title");
const statusDetail = requiredElement<HTMLElement>("#status-detail");
const convertModeButton = requiredElement<HTMLButtonElement>("#convert-mode");
const actionsModeButton = requiredElement<HTMLButtonElement>("#actions-mode");
const openOverlayButton = requiredElement<HTMLButtonElement>("#open-overlay");
const openSettingsButton = requiredElement<HTMLButtonElement>("#open-settings");
const settingsDialog = requiredElement<HTMLDialogElement>("#settings-dialog");
const settingsForm = requiredElement<HTMLFormElement>("#settings-form");
const convertModifierSelect = requiredElement<HTMLSelectElement>("#convert-modifier");
const actionsModifierPreview = requiredElement<HTMLElement>("#actions-modifier-preview");
const settingsFeedback = requiredElement<HTMLElement>("#settings-feedback");
const convertKey = requiredElement<HTMLElement>("#convert-key");
const actionsKey = requiredElement<HTMLElement>("#actions-key");
const convertKeyHelp = requiredElement<HTMLElement>("#convert-key-help");
const actionsKeyHelp = requiredElement<HTMLElement>("#actions-key-help");
const globalStatus = requiredElement<HTMLElement>("#global-status");
const selectionPreview = requiredElement<HTMLElement>("#selection-preview");
const selectionThumbnail = requiredElement<HTMLImageElement>("#selection-thumbnail");
const selectionExtension = requiredElement<HTMLElement>("#selection-extension");
const selectionName = requiredElement<HTMLElement>("#selection-name");
const selectionMore = requiredElement<HTMLElement>("#selection-more");

let selectedPaths: string[] = [];
let analysis: AnalyzeResponse | null = null;
let hoveredSector: number | null = null;
let processing = false;
let menuMode: MenuMode = "convert";
let conversionTargets: string[] = [];
let overlayHideTimer: number | null = null;
let dropCommitted = false;
let modifierSettings: ModifierSettings = {
  convertModifier: "shift",
  actionsModifier: "control",
};
let dragIntegrationStatus: DragIntegrationStatus | null = null;

const segments: SVGPathElement[] = [];
const segmentLabels: SVGTextElement[] = [];
const labelPositions: Point[] = [];
for (let index = 0; index < 6; index += 1) {
  const startAngle = -120 + index * 60;
  const endAngle = startAngle + 60;
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", ringSegmentPath(82, 178, startAngle, endAngle));
  path.dataset.index = String(index);
  path.classList.add("wheel-segment");
  path.addEventListener("click", () => {
    if (processing || selectedPaths.length === 0) return;
    hoveredSector = index;
    updateSegmentState();
    void commitDrop();
  });
  segmentGroup.append(path);
  segments.push(path);

  const labelPosition = polarPoint(132, startAngle + 30);
  labelPositions.push(labelPosition);
  const label = document.createElementNS("http://www.w3.org/2000/svg", "text");
  label.setAttribute("x", String(labelPosition.x));
  label.setAttribute("y", String(labelPosition.y));
  label.setAttribute("text-anchor", "middle");
  label.classList.add("segment-label");
  segmentGroup.append(label);
  segmentLabels.push(label);
}

renderMenuMode();

if (isOverlay && !runningInTauri) {
  selectedPaths = ["/home/user/Documents/example.docx"];
  analysis = {
    files: [{
      path: selectedPaths[0],
      name: "example.docx",
      kind: "document",
      targets: ["pdf"],
    }],
    commonTargets: ["pdf"],
  };
  stage.classList.add("is-dragging");
  centerIdle.classList.add("is-hidden");
  centerFiles.classList.remove("is-hidden");
  renderSelectionPreview(analysis);
  updateSegmentState();
}

convertModeButton.addEventListener("click", () => setMenuMode("convert"));
actionsModeButton.addEventListener("click", () => setMenuMode("actions"));
openOverlayButton.addEventListener("click", () => {
  void invoke("show_overlay", { mode: menuMode });
});
openSettingsButton.addEventListener("click", () => {
  convertModifierSelect.value = modifierSettings.convertModifier;
  renderSettingsPreview();
  settingsFeedback.textContent = "";
  settingsDialog.showModal();
});
convertModifierSelect.addEventListener("change", renderSettingsPreview);
document.querySelectorAll<HTMLElement>("[data-close-settings]").forEach((button) => {
  button.addEventListener("click", () => settingsDialog.close());
});
settingsForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void saveModifierSettings();
});
window.addEventListener("keydown", (event) => {
  const pressed = event.key === "Shift" ? "shift" : event.key === "Control" ? "control" : null;
  if (pressed === modifierSettings.convertModifier) setMenuMode("convert");
  if (pressed === modifierSettings.actionsModifier) setMenuMode("actions");
  if (event.key === "Escape" && isOverlay) void hideSystemOverlay();
});

if (runningInTauri) {
  await listen<MenuMode>("overlay-mode", (event) => {
    setMenuMode(event.payload, true);
  });
  await listen<ExternalSelection>("overlay-selection", (event) => {
    dropCommitted = false;
    void loadExternalSelection(event.payload.paths);
  });
  await listen<Point>("overlay-pointer", (event) => {
    if (isOverlay && !processing) updatePointer(event.payload);
  });
  await listen<Point>("overlay-release", (event) => {
    if (!isOverlay || processing || dropCommitted) return;
    updatePointer(event.payload);
    void commitDrop();
  });
  await listen<DragIntegrationStatus>("drag-integration-status", (event) => {
    renderDragIntegrationStatus(event.payload);
  });
  await listen<ModifierSettings>("modifier-settings", (event) => {
    applyModifierSettings(event.payload);
  });

  applyModifierSettings(await invoke<ModifierSettings>("get_modifier_settings"));
  const status = await invoke<DragIntegrationStatus>("drag_integration_status");
  renderDragIntegrationStatus(status);
  if (isOverlay) {
    const initialMode = await invoke<MenuMode>("current_overlay_mode");
    setMenuMode(initialMode, true);
    const initialSelection = await invoke<ExternalSelection>("current_overlay_selection");
    if (initialSelection.paths.length > 0) {
      await loadExternalSelection(initialSelection.paths);
    }
  }

  await getCurrentWebview().onDragDropEvent(async (event) => {
    if (processing) return;

    switch (event.payload.type) {
      case "enter":
        cancelOverlayHide();
        dropCommitted = false;
        selectedPaths = event.payload.paths;
        stage.classList.add("is-dragging");
        updatePointer(event.payload.position);
        await analyzeSelection(selectedPaths);
        break;
      case "over":
        updatePointer(event.payload.position);
        break;
      case "drop":
        selectedPaths = event.payload.paths;
        updatePointer(event.payload.position);
        await commitDrop();
        break;
      case "leave":
        if (isOverlay) {
          hoveredSector = null;
          updateSegmentState();
        } else {
          resetDragState();
        }
        break;
    }
  });
}

async function loadExternalSelection(paths: string[]): Promise<void> {
  if (processing) return;
  dropCommitted = false;
  if (paths.length === 0) {
    resetDragState();
    return;
  }
  cancelOverlayHide();
  selectedPaths = paths;
  hoveredSector = null;
  stage.classList.add("is-dragging");
  await analyzeSelection(selectedPaths);
}

function setMenuMode(mode: MenuMode, force = false): void {
  if (processing || (!force && menuMode === mode)) return;
  menuMode = mode;
  hoveredSector = null;
  renderMenuMode();

  if (selectedPaths.length > 0) {
    if (runningInTauri) {
      void analyzeSelection(selectedPaths);
    } else if (analysis) {
      renderSelectionPreview(analysis);
    }
  } else {
    setIdleStatus();
  }
}

function renderMenuMode(): void {
  const actionMode = menuMode === "actions";
  convertModeButton.classList.toggle("active", !actionMode);
  actionsModeButton.classList.toggle("active", actionMode);
  convertModeButton.setAttribute("aria-pressed", String(!actionMode));
  actionsModeButton.setAttribute("aria-pressed", String(actionMode));
  wheel.classList.toggle("is-actions", actionMode);
  wheel.setAttribute(
    "aria-label",
    actionMode ? "Dosya işlemleri" : "Dönüştürme hedefleri",
  );

  const labels = actionMode
    ? actionLabels
    : Array.from({ length: 6 }, (_, index) => {
        const target = conversionTargets[index];
        return target ? [target.toUpperCase()] : [];
      });
  labels.forEach((lines, index) => {
    const label = segmentLabels[index];
    const labelPosition = labelPositions[index];
    label.replaceChildren();
    label.setAttribute("y", String(labelPosition.y - (lines.length === 2 ? 7 : 0)));
    for (const [lineIndex, line] of lines.entries()) {
      const tspan = document.createElementNS("http://www.w3.org/2000/svg", "tspan");
      tspan.setAttribute("x", String(labelPosition.x));
      tspan.setAttribute("dy", lineIndex === 0 ? "0" : "14");
      tspan.textContent = line;
      label.append(tspan);
    }
  });
  updateSegmentState();
}

async function analyzeSelection(paths: string[]): Promise<AnalyzeResponse | null> {
  try {
    const response = await invoke<AnalyzeResponse>("analyze_files", { paths });
    analysis = response;
    conversionTargets = response.commonTargets.slice(0, 6);
    renderMenuMode();
    centerIdle.classList.add("is-hidden");
    centerFiles.classList.remove("is-hidden");
    fileCount.textContent = String(response.files.length);
    renderSelectionPreview(response);

    if (menuMode === "actions") {
      setStatus("↓", `${response.files.length} öğe hazır`, "İstediğin dosya işleminin üzerine bırak.", "ready");
    } else {
      const unsupported = response.files.filter((file) => file.kind === "unsupported");
      if (unsupported.length > 0) {
        setStatus("!", "Desteklenmeyen seçim", `${unsupported[0].name} henüz dönüştürülemiyor.`, "error");
      } else if (response.commonTargets.length > 0) {
        const targets = conversionTargets.map((target) => target.toUpperCase()).join(", ");
        setStatus("↓", `${response.files.length} dosya hazır`, `${targets} hedeflerinden birine bırak.`, "ready");
      } else {
        setStatus("!", "Ortak hedef bulunamadı", "Seçilen dosyalar birlikte dönüştürülemiyor.", "error");
      }
    }
    updateSegmentState();
    return response;
  } catch (error) {
    setStatus("!", "Dosyalar incelenemedi", String(error), "error");
    return null;
  }
}

async function handleDrop(): Promise<void> {
  const currentAnalysis = analysis ?? (await analyzeSelection(selectedPaths));
  if (!currentAnalysis) {
    resetDragState(false);
    return;
  }

  if (menuMode === "actions") {
    await handleActionDrop();
  } else {
    await handleConversionDrop(currentAnalysis);
  }
}

async function commitDrop(): Promise<void> {
  if (processing || dropCommitted) return;
  dropCommitted = true;
  if (selectedPaths.length === 0) {
    resetDragState(false);
    if (isOverlay) scheduleOverlayHide(250);
    return;
  }
  await handleDrop();
  if (isOverlay && !processing) scheduleOverlayHide(900);
}

async function handleConversionDrop(currentAnalysis: AnalyzeResponse): Promise<void> {
  const target = hoveredSector === null ? null : conversionTargets[hoveredSector];
  const canConvert = target !== null && target !== undefined
    && currentAnalysis.commonTargets.includes(target);
  if (!target || !canConvert) {
    setStatus("↗", "Bir dönüşüm hedefinin üzerine bırak", "Etiketli ve etkin dilimlerden birini hedefle.", "notice");
    resetDragState(false);
    return;
  }

  const targetLabel = target.toUpperCase();
  beginOperation(`${targetLabel} oluşturuluyor`, `${selectedPaths.length} dosya yerel olarak işleniyor.`);
  try {
    const response = await invoke<ConversionResponse>("convert_files", {
      paths: selectedPaths,
      target,
    });
    const outputText = response.outputs.length === 1
      ? fileName(response.outputs[0])
      : `${response.outputs.length} ${targetLabel} çıktısı oluşturuldu`;
    setStatus("✓", "Dönüşüm tamamlandı", outputText, "success");
  } catch (error) {
    setStatus("!", "Dönüşüm başarısız", String(error), "error");
  } finally {
    finishOperation();
  }
}

async function handleActionDrop(): Promise<void> {
  const action = hoveredSector === null ? null : actionBySector[hoveredSector];
  if (!action) {
    setStatus("↗", "Bir işlemin üzerine bırak", "Etiketli dilimlerden birini hedefle.", "notice");
    resetDragState(false);
    return;
  }

  const paths = [...selectedPaths];
  beginOperation("İşlem hazırlanıyor", `${paths.length} öğe yerel olarak işleniyor.`);
  try {
    switch (action) {
      case "copy-path":
        await writeText(paths.join("\n"));
        setStatus("✓", "Yol panoya kopyalandı", itemCount(paths.length), "success");
        break;
      case "copy":
        await transferWithFolderPicker(paths, "copy");
        break;
      case "move":
        await transferWithFolderPicker(paths, "move");
        break;
      case "reveal":
        await revealItemInDir(paths);
        setStatus("✓", "Dosya yöneticisinde gösterildi", itemCount(paths.length), "success");
        break;
      case "terminal":
        await invoke("open_terminal", { paths });
        setStatus("✓", "Terminal açıldı", fileName(paths[0]), "success");
        break;
    }
  } catch (error) {
    setStatus("!", "Dosya işlemi başarısız", String(error), "error");
  } finally {
    finishOperation();
  }
}

async function transferWithFolderPicker(
  paths: string[],
  operation: "copy" | "move",
): Promise<void> {
  const destination = await open({
    directory: true,
    multiple: false,
    title: operation === "copy" ? "Kopyalanacak klasörü seç" : "Taşınacak klasörü seç",
  });
  if (!destination) {
    setStatus("×", "İşlem iptal edildi", "Hedef klasör seçilmedi.", "notice");
    return;
  }

  setStatus(
    "…",
    operation === "copy" ? "Kopyalanıyor" : "Taşınıyor",
    `${paths.length} öğe yerel olarak işleniyor.`,
    "working",
  );
  const response = await invoke<TransferResponse>("transfer_files", {
    paths,
    destination,
    operation,
  });
  setStatus(
    "✓",
    operation === "copy" ? "Kopyalama tamamlandı" : "Taşıma tamamlandı",
    itemCount(response.outputs.length),
    "success",
  );
}

function beginOperation(title: string, detail: string): void {
  processing = true;
  stage.classList.remove("is-dragging");
  stage.classList.add("is-converting");
  setStatus("…", title, detail, "working");
}

function finishOperation(): void {
  processing = false;
  stage.classList.remove("is-converting");
  selectedPaths = [];
  analysis = null;
  conversionTargets = [];
  hoveredSector = null;
  renderMenuMode();
  updateSegmentState();
  centerIdle.classList.remove("is-hidden");
  centerFiles.classList.add("is-hidden");
  clearSelectionPreview();
  if (isOverlay) scheduleOverlayHide(900);
}

function updatePointer(position: Point): void {
  const scale = window.devicePixelRatio || 1;
  const rect = wheel.getBoundingClientRect();
  const x = position.x / scale - (rect.left + rect.width / 2);
  const y = position.y / scale - (rect.top + rect.height / 2);
  const radius = Math.hypot(x, y) * (420 / rect.width);

  if (radius < 82 || radius > 178) {
    hoveredSector = null;
  } else {
    const angle = Math.atan2(y, x) * (180 / Math.PI);
    hoveredSector = Math.floor(((angle + 120 + 360) % 360) / 60);
  }
  updateSegmentState();
}

function updateSegmentState(): void {
  const hasSelection = selectedPaths.length > 0;
  for (const segment of segments) {
    const index = Number(segment.dataset.index);
    const target = conversionTargets[index];
    const configured = menuMode === "convert" ? target !== undefined : actionBySector[index] !== null;
    const canConvert = target !== undefined && (analysis?.commonTargets.includes(target) ?? false);
    const available = configured && (menuMode === "actions" ? hasSelection : canConvert);
    segment.classList.toggle("target", configured);
    segment.classList.toggle("placeholder", !configured);
    segment.classList.toggle("is-available", available);
    segment.classList.toggle("is-hovered", available && index === hoveredSector);
  }
}

function resetDragState(resetStatus = true): void {
  stage.classList.remove("is-dragging");
  selectedPaths = [];
  analysis = null;
  conversionTargets = [];
  hoveredSector = null;
  renderMenuMode();
  updateSegmentState();
  centerIdle.classList.remove("is-hidden");
  centerFiles.classList.add("is-hidden");
  clearSelectionPreview();
  if (resetStatus) setIdleStatus();
}

function setIdleStatus(): void {
  if (menuMode === "actions") {
    setStatus("→", "Dosya veya klasör sürükle", "İşlem dilimlerinden birinin üzerine bırak.", "idle");
  } else {
    setStatus("→", "Dosyalarını buraya sürükle", "Uygun dönüşüm hedefleri otomatik olarak gösterilir.", "idle");
  }
}

function setStatus(icon: string, title: string, detail: string, state: string): void {
  statusIcon.textContent = icon;
  statusTitle.textContent = title;
  statusDetail.textContent = detail;
  stage.dataset.state = state;
}

function renderDragIntegrationStatus(status: DragIntegrationStatus): void {
  dragIntegrationStatus = status;
  globalStatus.classList.toggle("is-ready", status.ready);
  globalStatus.classList.toggle("is-error", !status.ready);
  globalStatus.textContent = `Dosyayı sürüklerken ${modifierLabel(modifierSettings.convertModifier)}: dönüştür • ${modifierLabel(modifierSettings.actionsModifier)}: dosya işlemleri`;
  globalStatus.title = status.ready
    ? `Dosya sürükleme: ${status.convertShortcut} / ${status.actionsShortcut} — ${status.detail}`
    : `${status.backend} — ${status.detail}`;
}

function modifierLabel(modifier: ModifierKey): string {
  return modifier === "shift" ? "Shift" : "Ctrl";
}

function applyModifierSettings(settings: ModifierSettings): void {
  modifierSettings = settings;
  convertModifierSelect.value = settings.convertModifier;
  renderSettingsPreview();
  convertKey.textContent = settings.convertModifier === "shift" ? "⇧" : "Ctrl";
  actionsKey.textContent = settings.actionsModifier === "shift" ? "⇧" : "Ctrl";
  convertKeyHelp.textContent = `${modifierLabel(settings.convertModifier)} ile seç`;
  actionsKeyHelp.textContent = `${modifierLabel(settings.actionsModifier)} ile seç`;
  if (dragIntegrationStatus) renderDragIntegrationStatus(dragIntegrationStatus);
}

function renderSettingsPreview(): void {
  const actionsModifier: ModifierKey = convertModifierSelect.value === "control" ? "shift" : "control";
  actionsModifierPreview.textContent = modifierLabel(actionsModifier);
}

async function saveModifierSettings(): Promise<void> {
  const convertModifier = convertModifierSelect.value as ModifierKey;
  settingsFeedback.textContent = "Kaydediliyor…";
  try {
    const settings = runningInTauri
      ? await invoke<ModifierSettings>("set_modifier_settings", { convertModifier })
      : {
          convertModifier,
          actionsModifier: convertModifier === "shift" ? "control" : "shift",
        } satisfies ModifierSettings;
    applyModifierSettings(settings);
    settingsFeedback.textContent = "Tuşlar güncellendi.";
    window.setTimeout(() => settingsDialog.close(), 450);
  } catch (error) {
    settingsFeedback.textContent = `Ayar kaydedilemedi: ${String(error)}`;
  }
}

function scheduleOverlayHide(delay: number): void {
  cancelOverlayHide();
  overlayHideTimer = window.setTimeout(() => {
    void hideSystemOverlay();
  }, delay);
}

function cancelOverlayHide(): void {
  if (overlayHideTimer !== null) {
    window.clearTimeout(overlayHideTimer);
    overlayHideTimer = null;
  }
}

async function hideSystemOverlay(): Promise<void> {
  cancelOverlayHide();
  if (runningInTauri) await invoke("hide_overlay");
}

function ringSegmentPath(inner: number, outer: number, start: number, end: number): string {
  const outerStart = polarPoint(outer, start);
  const outerEnd = polarPoint(outer, end);
  const innerEnd = polarPoint(inner, end);
  const innerStart = polarPoint(inner, start);
  return [
    `M ${outerStart.x} ${outerStart.y}`,
    `A ${outer} ${outer} 0 0 1 ${outerEnd.x} ${outerEnd.y}`,
    `L ${innerEnd.x} ${innerEnd.y}`,
    `A ${inner} ${inner} 0 0 0 ${innerStart.x} ${innerStart.y}`,
    "Z",
  ].join(" ");
}

function polarPoint(radius: number, angle: number): Point {
  const radians = angle * (Math.PI / 180);
  return {
    x: Math.round(Math.cos(radians) * radius * 1000) / 1000,
    y: Math.round(Math.sin(radians) * radius * 1000) / 1000,
  };
}

function fileName(path: string): string {
  return path.split(/[\\/]/).at(-1) ?? path;
}

function itemCount(count: number): string {
  return `${count} öğe`;
}

function renderSelectionPreview(response: AnalyzeResponse): void {
  const first = response.files[0];
  if (!first) {
    clearSelectionPreview();
    return;
  }

  const extension = fileExtension(first.name);
  selectionName.textContent = first.name;
  selectionExtension.textContent = extension || "DOSYA";
  selectionMore.textContent = response.files.length > 1 ? `+${response.files.length - 1}` : "";
  selectionPreview.classList.add("is-visible");

  if (runningInTauri && isPreviewableImage(extension)) {
    selectionThumbnail.src = convertFileSrc(first.path);
    selectionThumbnail.classList.add("is-visible");
    selectionExtension.classList.add("is-hidden");
  } else {
    selectionThumbnail.removeAttribute("src");
    selectionThumbnail.classList.remove("is-visible");
    selectionExtension.classList.remove("is-hidden");
  }
}

function clearSelectionPreview(): void {
  selectionPreview.classList.remove("is-visible");
  selectionThumbnail.removeAttribute("src");
  selectionThumbnail.classList.remove("is-visible");
  selectionExtension.classList.remove("is-hidden");
  selectionName.textContent = "";
  selectionMore.textContent = "";
}

function fileExtension(name: string): string {
  const extension = name.split(".").at(-1);
  return extension && extension !== name ? extension.toUpperCase() : "";
}

function isPreviewableImage(extension: string): boolean {
  return ["AVIF", "GIF", "HEIC", "JPEG", "JPG", "PNG", "TIFF", "WEBP"].includes(extension);
}

function requiredElement<T extends Element>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`Required element was not found: ${selector}`);
  return element;
}
