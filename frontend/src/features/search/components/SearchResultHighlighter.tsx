import React from 'react';

interface SearchResultHighlighterProps {
  text: string;
  query: string;
}

const SearchResultHighlighter = ({
  text,
  query,
}: SearchResultHighlighterProps) => {
  if (!query) return <>{text}</>;

  const parts = text.split(new RegExp(`(${query})`, 'gi'));

  return (
    <span>
      {parts.map((part, i) =>
        part.toLowerCase() === query.toLowerCase() ? (
          <mark key={i} className="bg-yellow-200 text-black rounded-sm">
            {part}
          </mark>
        ) : (
          part
        )
      )}
    </span>
  );
};

export { SearchResultHighlighter };
