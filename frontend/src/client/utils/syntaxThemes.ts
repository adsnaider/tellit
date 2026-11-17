type TokenStyle = {
  color?: string;
  fontStyle?: 'italic' | 'normal';
  fontWeight?: 'bold' | 'normal';
};

type TokenPalette = Record<string, TokenStyle>;

function makePalette(colors: {
  plain: string;
  comment: string;
  keyword: string;
  string: string;
  number: string;
  function: string;
  className: string;
  operator: string;
  punctuation: string;
  property: string;
  tag: string;
  attrName: string;
  regex: string;
  deleted: string;
  inserted: string;
}): TokenPalette {
  return {
    comment: { color: colors.comment, fontStyle: 'italic' },
    prolog: { color: colors.comment },
    doctype: { color: colors.comment },
    cdata: { color: colors.comment },
    punctuation: { color: colors.punctuation },
    operator: { color: colors.operator },
    keyword: { color: colors.keyword },
    boolean: { color: colors.number },
    number: { color: colors.number },
    constant: { color: colors.number },
    symbol: { color: colors.number },
    builtin: { color: colors.className },
    'class-name': { color: colors.className },
    'maybe-class-name': { color: colors.className },
    function: { color: colors.function },
    'function-variable': { color: colors.function },
    string: { color: colors.string },
    char: { color: colors.string },
    'template-punctuation': { color: colors.string },
    'attr-value': { color: colors.string },
    tag: { color: colors.tag },
    selector: { color: colors.tag },
    'attr-name': { color: colors.attrName },
    property: { color: colors.property },
    variable: { color: colors.plain },
    parameter: { color: colors.plain },
    regex: { color: colors.regex },
    important: { color: colors.keyword, fontWeight: 'bold' },
    bold: { fontWeight: 'bold' },
    italic: { fontStyle: 'italic' },
    deleted: { color: colors.deleted },
    inserted: { color: colors.inserted },
    atrule: { color: colors.keyword },
    namespace: { color: colors.className },
    entity: { color: colors.function },
    url: { color: colors.string },
  };
}

const THEME_PALETTES: Record<string, TokenPalette> = {
  vsDark: makePalette({
    plain: '#d4d4d4',
    comment: '#6a9955',
    keyword: '#569cd6',
    string: '#ce9178',
    number: '#b5cea8',
    function: '#dcdcaa',
    className: '#4ec9b0',
    operator: '#d4d4d4',
    punctuation: '#d4d4d4',
    property: '#9cdcfe',
    tag: '#569cd6',
    attrName: '#9cdcfe',
    regex: '#d16969',
    deleted: '#ce9178',
    inserted: '#b5cea8',
  }),
  oneDark: makePalette({
    plain: '#abb2bf',
    comment: '#5c6370',
    keyword: '#c678dd',
    string: '#98c379',
    number: '#d19a66',
    function: '#61afef',
    className: '#e5c07b',
    operator: '#56b6c2',
    punctuation: '#abb2bf',
    property: '#e06c75',
    tag: '#e06c75',
    attrName: '#d19a66',
    regex: '#56b6c2',
    deleted: '#e06c75',
    inserted: '#98c379',
  }),
  gruvboxMaterialDark: makePalette({
    plain: '#d4be98',
    comment: '#928374',
    keyword: '#ea6962',
    string: '#a9b665',
    number: '#d3869b',
    function: '#a9b665',
    className: '#d8a657',
    operator: '#e78a4e',
    punctuation: '#d4be98',
    property: '#7daea3',
    tag: '#ea6962',
    attrName: '#d8a657',
    regex: '#89b482',
    deleted: '#ea6962',
    inserted: '#a9b665',
  }),
  nightOwl: makePalette({
    plain: '#d6deeb',
    comment: '#637777',
    keyword: '#c792ea',
    string: '#ecc48d',
    number: '#f78c6c',
    function: '#82aaff',
    className: '#ffcb8b',
    operator: '#7fdbca',
    punctuation: '#c792ea',
    property: '#80cbc4',
    tag: '#caece6',
    attrName: '#addb67',
    regex: '#5ca7e4',
    deleted: '#ef5350',
    inserted: '#addb67',
  }),
  dracula: makePalette({
    plain: '#f8f8f2',
    comment: '#6272a4',
    keyword: '#ff79c6',
    string: '#f1fa8c',
    number: '#bd93f9',
    function: '#50fa7b',
    className: '#8be9fd',
    operator: '#ff79c6',
    punctuation: '#f8f8f2',
    property: '#66d9ef',
    tag: '#ff79c6',
    attrName: '#50fa7b',
    regex: '#f1fa8c',
    deleted: '#ff5555',
    inserted: '#50fa7b',
  }),
  okaidia: makePalette({
    plain: '#f8f8f2',
    comment: '#8292a2',
    keyword: '#66d9ef',
    string: '#a6e22e',
    number: '#ae81ff',
    function: '#e6db74',
    className: '#a6e22e',
    operator: '#f8f8f2',
    punctuation: '#f8f8f2',
    property: '#f92672',
    tag: '#f92672',
    attrName: '#a6e22e',
    regex: '#fd971f',
    deleted: '#f92672',
    inserted: '#a6e22e',
  }),
  github: makePalette({
    plain: '#24292e',
    comment: '#6a737d',
    keyword: '#d73a49',
    string: '#032f62',
    number: '#005cc5',
    function: '#6f42c1',
    className: '#e36209',
    operator: '#d73a49',
    punctuation: '#24292e',
    property: '#005cc5',
    tag: '#22863a',
    attrName: '#6f42c1',
    regex: '#032f62',
    deleted: '#b31d28',
    inserted: '#22863a',
  }),
  vsLight: makePalette({
    plain: '#000000',
    comment: '#008000',
    keyword: '#0000ff',
    string: '#a31515',
    number: '#098658',
    function: '#795e26',
    className: '#267f99',
    operator: '#000000',
    punctuation: '#000000',
    property: '#001080',
    tag: '#800000',
    attrName: '#ff0000',
    regex: '#811f3f',
    deleted: '#a31515',
    inserted: '#098658',
  }),
  oneLight: makePalette({
    plain: '#383a42',
    comment: '#a0a1a7',
    keyword: '#a626a4',
    string: '#50a14f',
    number: '#986801',
    function: '#4078f2',
    className: '#c18401',
    operator: '#0184bc',
    punctuation: '#383a42',
    property: '#e45649',
    tag: '#e45649',
    attrName: '#986801',
    regex: '#0184bc',
    deleted: '#e45649',
    inserted: '#50a14f',
  }),
  gruvboxMaterialLight: makePalette({
    plain: '#654735',
    comment: '#928374',
    keyword: '#c14a4a',
    string: '#6c782e',
    number: '#945e80',
    function: '#6c782e',
    className: '#b47109',
    operator: '#c35e0a',
    punctuation: '#654735',
    property: '#45707a',
    tag: '#c14a4a',
    attrName: '#b47109',
    regex: '#4c7a5d',
    deleted: '#c14a4a',
    inserted: '#6c782e',
  }),
  nightOwlLight: makePalette({
    plain: '#403f53',
    comment: '#989fb1',
    keyword: '#994cc3',
    string: '#c96765',
    number: '#aa0982',
    function: '#4876d6',
    className: '#111111',
    operator: '#0c969b',
    punctuation: '#994cc3',
    property: '#0c969b',
    tag: '#994cc3',
    attrName: '#4876d6',
    regex: '#c96765',
    deleted: '#de3d3b',
    inserted: '#08916a',
  }),
};

export function getTokenStyleString(types: readonly string[], syntaxTheme = 'vsDark'): string {
  const palette = THEME_PALETTES[syntaxTheme] ?? THEME_PALETTES.vsDark;
  if (!palette || types.length === 0) return '';

  let color: string | undefined;
  let fontStyle: string | undefined;
  let fontWeight: string | undefined;

  for (const type of types) {
    const style = palette[type];
    if (!style) continue;
    if (style.color) color = style.color;
    if (style.fontStyle) fontStyle = style.fontStyle;
    if (style.fontWeight) fontWeight = style.fontWeight;
  }

  const declarations: string[] = [];
  if (color) declarations.push(`color:${color}`);
  if (fontStyle) declarations.push(`font-style:${fontStyle}`);
  if (fontWeight) declarations.push(`font-weight:${fontWeight}`);
  return declarations.join(';');
}
