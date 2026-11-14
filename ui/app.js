// 화면은 보여 주고 누르는 일만 한다. 파일 읽기, 변환, 저장은 모두 러스트가 한다.
// 화면은 파일 경로를 모르고, 러스트가 매긴 번호로만 부탁한다.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const items = new Map(); // 번호 -> 파일 상태
let queue = Promise.resolve(); // 미리 만들기는 한 번에 하나씩

const $ = (id) => document.getElementById(id);

function fmt(bytes) {
  if (bytes < 1000) return `${bytes} B`;
  if (bytes < 1000 * 1000) return `${(bytes / 1000).toFixed(1)} KB`;
  return `${(bytes / 1000 / 1000).toFixed(1)} MB`;
}

function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text; // 파일 이름은 글자로만 넣는다
  return node;
}

function prepareLater(item) {
  item.ready = queue.then(async () => {
    if (!items.has(item.info.id)) return;
    item.state = "preparing";
    render();
    try {
      item.prepared = await invoke("prepare", { id: item.info.id, target: item.target.id });
      item.state = "ready";
    } catch (err) {
      item.state = "error";
      item.error = String(err);
    }
    render();
  });
  queue = item.ready;
}

async function convertAll(targetId) {
  const todo = [...items.values()].filter(
    (it) => it.target && it.target.id === targetId && it.state !== "saved" && it.state !== "error",
  );
  for (const item of todo) {
    await item.ready;
    if (item.state !== "ready") continue;
    item.state = "saving";
    render();
    try {
      item.saved = await invoke("save", { id: item.info.id });
      item.state = "saved";
    } catch (err) {
      item.state = "error";
      item.error = String(err);
    }
    render();
  }
}

function statusText(item) {
  const p = item.prepared;
  switch (item.state) {
    case "waiting":
      return ["기다리는 중", ""];
    case "preparing":
      return ["예상 크기 계산 중", ""];
    case "ready": {
      let text = `${item.target.label}로 바꾸면 예상 ${fmt(p.out_size)} (화질 ${p.quality})`;
      if (p.had_alpha) text += ", 투명한 부분은 검은색으로 채웁니다";
      if (p.distance !== null && p.distance > 1.0) text += ", 화질 100에서도 아주 작은 차이가 남을 수 있습니다";
      return [text, ""];
    }
    case "saving":
      return ["저장 중", ""];
    case "saved":
      return [`저장했습니다: ${item.saved.name} (${fmt(item.saved.size)})`, "done"];
    case "error":
      return [item.error, "error"];
    case "unsupported":
      return ["아직 바꿀 수 없는 종류입니다", ""];
    default:
      return ["", ""];
  }
}

function render() {
  const list = $("list");
  list.replaceChildren();
  for (const item of items.values()) {
    const li = el("li");
    li.append(el("span", "name", item.info.name));
    const kind = item.info.kind ? `${item.info.kind}, ${fmt(item.info.size)}` : "";
    if (kind) li.append(el("span", "meta", kind));
    const [text, cls] = statusText(item);
    li.append(el("span", `meta status ${cls}`, text));
    list.append(li);
  }

  // 바꿀 수 있는 형식마다 버튼 하나
  const groups = new Map();
  for (const item of items.values()) {
    if (!item.target || item.state === "error") continue;
    const g = groups.get(item.target.id) || { target: item.target, all: [], left: [] };
    g.all.push(item);
    if (item.state !== "saved") g.left.push(item);
    groups.set(item.target.id, g);
  }

  const actions = $("actions");
  actions.replaceChildren();
  const lines = [];
  for (const g of groups.values()) {
    const button = el("button", "", `${g.target.label}로 바꾸기 (${g.left.length}개)`);
    button.disabled = g.left.length === 0 || g.left.some((it) => it.state === "saving");
    button.addEventListener("click", () => convertAll(g.target.id));
    actions.append(button);

    const known = g.all.filter((it) => it.prepared);
    const before = known.reduce((s, it) => s + it.info.size, 0);
    const after = known.reduce((s, it) => s + it.prepared.out_size, 0);
    let line = `${g.target.label}: 원본 ${fmt(before)}, 예상 ${fmt(after)}`;
    if (known.length < g.all.length) line += ` (${known.length}/${g.all.length}개 계산함)`;
    lines.push(line);
  }
  $("summary").textContent = lines.join(" / ");
  $("ask").hidden = groups.size === 0;
  $("clear").hidden = items.size === 0;
}

listen("files-dropped", (event) => {
  for (const info of event.payload) {
    const item = { info, target: info.targets[0] || null, state: "waiting" };
    if (info.error) {
      item.state = "error";
      item.error = info.error;
    } else if (!item.target) {
      item.state = "unsupported";
    }
    items.set(info.id, item);
    if (item.state === "waiting") prepareLater(item);
  }
  render();
});

const drop = $("drop");
listen("tauri://drag-enter", () => drop.classList.add("over"));
listen("tauri://drag-leave", () => drop.classList.remove("over"));
listen("tauri://drag-drop", () => drop.classList.remove("over"));

$("clear").addEventListener("click", async () => {
  await invoke("clear");
  items.clear();
  render();
});
