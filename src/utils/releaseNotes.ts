/**
 * Reduce a GitHub Release body (Markdown) to a short plain-text digest that fits
 * in the updater's native dialog. `docs/releases/vX.Y.Z.md` is dozens of lines of
 * headings, bold labels and backticks — none of which means anything to the user
 * when dumped verbatim into a message box, so the dialog shows a few bullets and
 * points at the full notes instead.
 */

export interface ReleaseNotesDigest {
  /** Bullet lines joined with "\n"; empty when the notes hold no usable text. */
  text: string;
  /** True when part of the notes was left out. */
  truncated: boolean;
}

/** Bullets kept at most. */
const MAX_BULLETS = 4;
/** Characters allowed per bullet before it is cut at a word boundary. */
const MAX_BULLET_CHARS = 240;
/** A bullet only slightly over the limit is kept whole instead of cut mid-sentence. */
const BULLET_SLACK = 1.15;
/** Characters allowed for the whole digest. */
const MAX_TOTAL_CHARS = 700;

const HEADING = /^#{1,6}\s+(.*)$/;
const BULLET = /^\s*(?:[-*+]|\d+\.)\s+(.*)$/;

/** Section preferred over all others when present. */
const PREFERRED_HEADING = /(highlight|亮点)/i;
/** Sections never shown: they are about verification or link out to the diff. */
const SKIPPED_HEADING = /(validation|changelog|校验)/i;
/** Metadata lines of the release-notes template. */
const METADATA_LINE = /^(release date|full changelog|发布日期)/i;

interface Section {
  heading: string;
  bullets: string[];
}

function stripMarkdown(text: string): string {
  return text
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/__([^_]+)__/g, "$1")
    .replace(/(^|[\s(])\*([^*\n]+)\*/g, "$1$2")
    .replace(/(^|[\s(])_([^_\n]+)_/g, "$1$2")
    .replace(/\s+/g, " ")
    // `**标签：** 说明` loses the bold run and keeps a stray space after CJK punctuation.
    .replace(/([，。、；：！？])\s+/g, "$1")
    .trim();
}

/** Cut at a word boundary so the ellipsis never lands mid-word. */
function cutAtWord(text: string, limit: number): string {
  if (text.length <= limit) return text;
  const slice = text.slice(0, limit);
  const boundary = slice.search(/\s+\S*$/);
  const head = boundary > 0 ? slice.slice(0, boundary) : slice;
  return `${head.replace(/[\s,;:.、，。；：]+$/, "")}…`;
}

function parseSections(lines: string[]): { intro: string[]; sections: Section[] } {
  const intro: string[] = [];
  const sections: Section[] = [];
  let current: Section | null = null;

  for (const line of lines) {
    const heading = line.match(HEADING);
    if (heading) {
      current = { heading: stripMarkdown(heading[1]), bullets: [] };
      sections.push(current);
      continue;
    }

    const bullet = line.match(BULLET);
    if (!bullet) continue;

    const text = stripMarkdown(bullet[1]);
    if (!text) continue;
    (current?.bullets ?? intro).push(text);
  }

  return { intro, sections };
}

/** Last resort for notes without any bullet list. */
function plainParagraph(lines: string[]): string[] {
  const parts: string[] = [];
  for (const line of lines) {
    if (!line.trim() || HEADING.test(line) || BULLET.test(line)) continue;
    const text = stripMarkdown(line);
    if (!text || METADATA_LINE.test(text) || /^https?:\/\//.test(text)) continue;
    parts.push(text);
  }
  return parts.length > 0 ? [parts.join(" ")] : [];
}

function pickItems(lines: string[]): string[] {
  const { intro, sections } = parseSections(lines);

  const preferred = sections.find((section) => PREFERRED_HEADING.test(section.heading));
  if (preferred && preferred.bullets.length > 0) return preferred.bullets;

  const kept = sections.filter((section) => !SKIPPED_HEADING.test(section.heading));
  const keptBullets = kept.flatMap((section) => section.bullets);
  if (keptBullets.length > 0) return keptBullets;

  const allBullets = [...intro, ...sections.flatMap((section) => section.bullets)];
  return allBullets.length > 0 ? allBullets : plainParagraph(lines);
}

export function summarizeReleaseNotes(body: string | null | undefined): ReleaseNotesDigest {
  const lines = (body ?? "").replace(/\r\n?/g, "\n").split("\n");
  const items = pickItems(lines);

  const bullets: string[] = [];
  let budget = MAX_TOTAL_CHARS;
  let truncated = items.length > MAX_BULLETS;

  for (const item of items.slice(0, MAX_BULLETS)) {
    const short =
      item.length <= MAX_BULLET_CHARS * BULLET_SLACK
        ? item
        : cutAtWord(item, MAX_BULLET_CHARS);
    if (short !== item) truncated = true;
    if (short.length + 2 > budget) {
      truncated = true;
      break;
    }
    bullets.push(`• ${short}`);
    budget -= short.length + 2;
  }

  const text = bullets.join("\n");
  return { text, truncated: truncated && text.length > 0 };
}
