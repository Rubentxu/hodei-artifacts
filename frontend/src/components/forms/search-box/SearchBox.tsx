import React, { useState, useEffect, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { useSearchStore } from '@/features/search';
import { Input } from '../../ui/Input';
import { Search as SearchIcon, History } from 'lucide-react';

const SearchBox = () => {
  const { query, setQuery, history, addSearchToHistory } = useSearchStore();

  const navigate = useNavigate();
  const [isFocused, setIsFocused] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const handleSearch = (e: React.FormEvent) => {
    e.preventDefault();
    if (query.trim()) {
      addSearchToHistory(query);
      navigate(`/search?query=${encodeURIComponent(query.trim())}`);
      setIsFocused(false);
    }
  };

  const handleHistoryClick = (searchQuery: string) => {
    setQuery(searchQuery);
    addSearchToHistory(searchQuery);
    navigate(`/search?query=${encodeURIComponent(searchQuery.trim())}`);
    setIsFocused(false);
  };

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (
        containerRef.current &&
        !containerRef.current.contains(event.target as Node)
      ) {
        setIsFocused(false);
      }
    };
    document.addEventListener('mousedown', handleClickOutside);
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [containerRef]);

  return (
    <div ref={containerRef} className="relative w-full">
      <form onSubmit={handleSearch}>
        <Input
          value={query}
          onChange={e => setQuery(e.target.value)}
          onFocus={() => setIsFocused(true)}
          placeholder="Search artifacts..."
          className="pl-10"
          leftIcon={<SearchIcon size={18} className="text-muted-foreground" />}
        />
      </form>
      {isFocused && history.length > 0 && (
        <div className="absolute mt-1 w-full rounded-md bg-white shadow-lg z-10 border">
          <ul className="py-1">
            {history.map((item, index) => (
              <li
                key={index}
                className="px-3 py-2 text-sm text-gray-700 hover:bg-gray-100 cursor-pointer flex items-center gap-2"
                onMouseDown={() => handleHistoryClick(item)} // use onMouseDown to fire before onBlur
              >
                <History size={14} className="text-gray-400" />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
};

export { SearchBox };
