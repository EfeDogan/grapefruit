const modes = {
  convert: {
    labels: ["PDF", "DOCX", "ODT", "TXT", "HTML", "EPUB"],
    caption: "PDF'e bırak — dönüşüm otomatik başlar.",
    file: "DOCX",
  },
  actions: {
    labels: ["YOL", "KOPYALA", "TAŞI", "GÖSTER", "TERMİNAL", "—"],
    caption: "Control ile günlük dosya işlemlerine geç.",
    file: "3 ÖĞE",
  },
};

const segmentGroup = document.querySelector("#demo-segments");
const caption = document.querySelector("#demo-caption");
const demoFile = document.querySelector("#demo-file");
const modeButtons = [...document.querySelectorAll(".mode-button")];
const segments = [];

function point(radius, angle) {
  const radians = (angle * Math.PI) / 180;
  return { x: Math.cos(radians) * radius, y: Math.sin(radians) * radius };
}

function ringSegment(inner, outer, start, end) {
  const a = point(outer, start);
  const b = point(outer, end);
  const c = point(inner, end);
  const d = point(inner, start);
  return `M ${a.x} ${a.y} A ${outer} ${outer} 0 0 1 ${b.x} ${b.y} L ${c.x} ${c.y} A ${inner} ${inner} 0 0 0 ${d.x} ${d.y} Z`;
}

for (let index = 0; index < 6; index += 1) {
  const start = -120 + index * 60;
  const end = start + 60;
  const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
  path.setAttribute("d", ringSegment(82, 178, start, end));
  path.classList.add("demo-segment");
  if (index === 0) path.classList.add("active");
  segmentGroup.append(path);

  const labelPoint = point(132, start + 30);
  const label = document.createElementNS("http://www.w3.org/2000/svg", "text");
  label.setAttribute("x", String(labelPoint.x));
  label.setAttribute("y", String(labelPoint.y + 5));
  label.classList.add("demo-label");
  segmentGroup.append(label);
  segments.push({ path, label });
}

function setMode(mode) {
  const selected = modes[mode];
  segments.forEach(({ path, label }, index) => {
    label.textContent = selected.labels[index];
    path.classList.toggle("active", index === 0);
  });
  demoFile.textContent = selected.file;
  caption.textContent = selected.caption;
  modeButtons.forEach((button) => {
    const active = button.dataset.mode === mode;
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", String(active));
  });
}

modeButtons.forEach((button) => button.addEventListener("click", () => setMode(button.dataset.mode)));
setMode("convert");

const observer = new IntersectionObserver((entries) => {
  entries.forEach((entry) => {
    if (entry.isIntersecting) {
      entry.target.classList.add("is-visible");
      observer.unobserve(entry.target);
    }
  });
}, { threshold: 0.14 });

document.querySelectorAll(".reveal").forEach((element) => observer.observe(element));
