// SafeHTML tagged template. Adopted from raybird/plainvanillaweb (lib/html.js):
// values are HTML-escaped unless explicitly marked safe via unsafe().
export class SafeHTML {
  constructor(value) {
    this.value = value;
    this.__isSafe = true;
  }
  toString() {
    return this.value;
  }
}

export function escapeHTML(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

export function unsafe(value) {
  return new SafeHTML(String(value));
}

export function html(strings, ...values) {
  let out = '';
  strings.forEach((str, i) => {
    out += str;
    if (i < values.length) {
      const v = values[i];
      if (v && v.__isSafe) out += v.toString();
      else if (Array.isArray(v)) {
        out += v.map((x) => (x && x.__isSafe ? x.toString() : escapeHTML(x))).join('');
      } else out += escapeHTML(v);
    }
  });
  return new SafeHTML(out);
}
