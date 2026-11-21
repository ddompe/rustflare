import React from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './App'
import { loadSettings } from './settings'
import './styles.css'

// Saved preferences are loaded first so the UI starts with them (no flash of defaults).
loadSettings().finally(() =>
  createRoot(document.getElementById('root')!).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  ),
)
