import { useState } from 'react';
import SettingsPanel from './SettingsPanel';

function App() {
  const [activeTab, setActiveTab] = useState<'chat' | 'settings'>('settings');

  return (
    <div className="min-h-screen bg-muted text-foreground flex flex-col">
      {/* Header Navigation */}
      <header className="bg-white border-b border-border py-4 px-6 flex items-center justify-between shadow-sm sticky top-0 z-10">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 bg-primary text-white rounded-lg flex items-center justify-center font-bold text-xl">
            H
          </div>
          <h1 className="text-xl font-bold tracking-tight">Hakimi WebUI</h1>
        </div>
        
        <nav className="flex gap-2">
          <button
            onClick={() => setActiveTab('chat')}
            className={`px-4 py-2 rounded-md font-medium transition-colors ${
              activeTab === 'chat' 
                ? 'bg-primary/10 text-primary' 
                : 'text-secondary hover:bg-muted'
            }`}
          >
            Chat
          </button>
          <button
            onClick={() => setActiveTab('settings')}
            className={`px-4 py-2 rounded-md font-medium transition-colors ${
              activeTab === 'settings' 
                ? 'bg-primary/10 text-primary' 
                : 'text-secondary hover:bg-muted'
            }`}
          >
            Settings
          </button>
        </nav>
      </header>

      {/* Main Content Area */}
      <main className="flex-1 overflow-auto">
        {activeTab === 'settings' ? (
          <SettingsPanel />
        ) : (
          <div className="h-full flex items-center justify-center text-secondary flex-col gap-4">
            <span className="text-4xl">🐱</span>
            <p>Chat interface is under construction... 🚧</p>
          </div>
        )}
      </main>
    </div>
  );
}

export default App;