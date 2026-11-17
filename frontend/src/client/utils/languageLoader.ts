import { isPlainLanguage, isPrismLanguageLoaded } from './prism';

const loaded: Record<string, Promise<void>> = {};

const LANGUAGE_IMPORTS: Record<string, () => Promise<unknown>> = {
  typescript: () => import('prismjs/components/prism-typescript.js'),
  jsx: () => import('prismjs/components/prism-jsx.js'),
  tsx: async () => {
    await loadPrismLanguage('jsx');
    await loadPrismLanguage('typescript');
    return import('prismjs/components/prism-tsx.js');
  },
  json: () => import('prismjs/components/prism-json.js'),
  rust: () => import('prismjs/components/prism-rust.js'),
  go: () => import('prismjs/components/prism-go.js'),
  python: () => import('prismjs/components/prism-python.js'),
  yaml: () => import('prismjs/components/prism-yaml.js'),
  toml: () => import('prismjs/components/prism-toml.js'),
  markdown: () => import('prismjs/components/prism-markdown.js'),
  c: () => import('prismjs/components/prism-c.js'),
  cpp: async () => {
    await loadPrismLanguage('c');
    return import('prismjs/components/prism-cpp.js');
  },
  kotlin: () => import('prismjs/components/prism-kotlin.js'),
  swift: () => import('prismjs/components/prism-swift.js'),
  r: () => import('prismjs/components/prism-r.js'),
  lua: () => import('prismjs/components/prism-lua.js'),
  docker: () => import('prismjs/components/prism-docker.js'),
  makefile: () => import('prismjs/components/prism-makefile.js'),
  git: () => import('prismjs/components/prism-git.js'),
  nginx: () => import('prismjs/components/prism-nginx.js'),
  ini: () => import('prismjs/components/prism-ini.js'),
  diff: () => import('prismjs/components/prism-diff.js'),
  bash: () => import('prismjs/components/prism-bash.js'),
  sh: () => import('prismjs/components/prism-bash.js'),
  shell: () => import('prismjs/components/prism-bash.js'),
  php: async () => {
    // PHP requires markup-templating.
    await import('prismjs/components/prism-markup-templating.js');
    return import('prismjs/components/prism-php.js');
  },
  sql: () => import('prismjs/components/prism-sql.js'),
  ruby: () => import('prismjs/components/prism-ruby.js'),
  java: () => import('prismjs/components/prism-java.js'),
  scala: async () => {
    // Scala requires Java.
    await loadPrismLanguage('java');
    return import('prismjs/components/prism-scala.js');
  },
  solidity: () => import('prismjs/components/prism-solidity.js'),
  vim: () => import('prismjs/components/prism-vim.js'),
  dart: () => import('prismjs/components/prism-dart.js'),
  csharp: () => import('prismjs/components/prism-csharp.js'),
  protobuf: () => import('prismjs/components/prism-protobuf.js'),
  hcl: () => import('prismjs/components/prism-hcl.js'),
  perl: () => import('prismjs/components/prism-perl.js'),
  elixir: () => import('prismjs/components/prism-elixir.js'),
  nix: () => import('prismjs/components/prism-nix.js'),
  haskell: () => import('prismjs/components/prism-haskell.js'),
  clojure: () => import('prismjs/components/prism-clojure.js'),
  gdscript: () => import('prismjs/components/prism-gdscript.js'),
  groovy: () => import('prismjs/components/prism-groovy.js'),
  // Svelte grammar extends markup and embeds js/css (all bundled with core prismjs).
  svelte: () => import('prism-svelte'),
};

/**
 * Dynamically loads a PrismJS language grammar (and its dependencies) once.
 */
export function loadPrismLanguage(lang: string): Promise<void> {
  if (isPlainLanguage(lang) || isPrismLanguageLoaded(lang)) {
    return Promise.resolve();
  }

  if (!loaded[lang]) {
    const importFn = LANGUAGE_IMPORTS[lang];
    if (!importFn) {
      return Promise.reject(new Error(`Unsupported language: ${lang}`));
    }

    loaded[lang] = importFn()
      .then(() => void 0)
      .catch((err) => {
        delete loaded[lang];
        console.warn(`Failed to load language: ${lang}`, err);
        throw err;
      });
  }
  return loaded[lang];
}
