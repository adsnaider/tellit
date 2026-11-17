// TypeScript declarations for Prismjs core and dynamic language components
declare module 'prismjs' {
  export type Grammar = Record<string, unknown>;

  export interface Token {
    type: string;
    content: TokenStream;
    alias?: string | string[];
    length: number;
  }

  export type TokenStream = string | Token | Array<string | Token>;

  const Prism: {
    languages: Record<string, Grammar | undefined>;
    tokenize(text: string, grammar: Grammar): Array<string | Token>;
  };

  export default Prism;
}

declare module 'prismjs/components/*' {
  const value: unknown;
  export = value;
}

// Third-party Prism grammar plugin (registers Prism.languages.svelte on import).
declare module 'prism-svelte' {
  const value: unknown;
  export = value;
}
