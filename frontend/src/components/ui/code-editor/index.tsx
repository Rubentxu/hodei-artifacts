import React from 'react';
import Editor from 'react-simple-code-editor';
import { highlight, languages } from 'prismjs/components/prism-core';
import 'prismjs/components/prism-clike';
import 'prismjs/components/prism-javascript';
import 'prismjs/themes/prism.css'; // Example style, can be customized

// Simple Cedar language definition for PrismJS
// This is a basic definition and can be expanded
languages.cedar = {
  'comment': /#.*/,
  'string': {
    pattern: /"(?:\\.|[^\\"\r\n])*"/,
    greedy: true
  },
  'keyword': /\b(?:permit|forbid|when|unless|in|has|like|if|then|else)\b/,
  'boolean': /\b(?:true|false)\b/,
  'function': /\b(?:principal|action|resource|context)\b/,
  'operator': /[!*+\-\/<=>&|]+/,
  'punctuation': /::|;|,|\.|\(|\)|\[|\]|\{|\}/
};

interface CodeEditorProps {
  value: string;
  onValueChange: (value: string) => void;
  language?: string;
  padding?: number;
  style?: React.CSSProperties;
}

const CodeEditor = ({
  value,
  onValueChange,
  language = 'cedar',
  padding = 10,
  style,
}: CodeEditorProps) => {
  const highlightWithLineNumbers = (code: string) =>
    highlight(code, languages[language], language)
      .split('\n')
      .map((line, i) => `<span class='editor-line-number'>${i + 1}</span>${line}`)
      .join('\n');

  return (
    <div className="code-editor-container border rounded-md bg-white">
      <Editor
        value={value}
        onValueChange={onValueChange}
        highlight={highlightWithLineNumbers}
        padding={padding}
        className="editor"
        style={{
          fontFamily: '"Fira code", "Fira Mono", monospace',
          fontSize: 14,
          ...style,
        }}
      />
      <style jsx>{`
        .code-editor-container {
          position: relative;
          min-height: 100px;
        }
        .editor {
          counter-reset: line;
        }
        .editor-line-number {
          position: absolute;
          left: 0;
          width: 2em;
          text-align: right;
          opacity: 0.3;
          user-select: none;
        }
      `}</style>
    </div>
  );
};

export { CodeEditor };
