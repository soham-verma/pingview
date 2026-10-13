import { useState } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';

const MESSAGES = {
  no_key: 'Showing the offline map. A free MapTiler key adds detailed maps.',
  bad_key: 'MapTiler rejected the API key, so the offline map is showing.',
};
const DISMISSED_KEY = 'pingview.mapNoticeDismissed';

// A small, dismissible hint: the offline map always works, so a missing or
// rejected key is an upgrade suggestion, not an error.
export default function MapNotice({ status, onOpenSettings }) {
  const [dismissed, setDismissed] = useState(() => localStorage.getItem(DISMISSED_KEY) === status);
  if (!MESSAGES[status] || dismissed) return null;

  const dismiss = () => {
    localStorage.setItem(DISMISSED_KEY, status);
    setDismissed(true);
  };

  return (
    <div className="map-notice" role="status">
      <span>{MESSAGES[status]}</span>
      {status === 'no_key' && (
        <a
          href="https://cloud.maptiler.com/account/keys/"
          className="link-btn"
          onClick={(e) => {
            e.preventDefault();
            openUrl('https://cloud.maptiler.com/account/keys/');
          }}
        >
          Get a key
        </a>
      )}
      <button type="button" className="link-btn" onClick={onOpenSettings}>
        Settings
      </button>
      <button type="button" className="link-btn notice-close" onClick={dismiss} aria-label="Dismiss">
        ✕
      </button>
    </div>
  );
}
