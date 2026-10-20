import { openUrl } from '@tauri-apps/plugin-opener';
import { LinkButton, Sheet, SheetRow, SheetSection } from './Sheet.jsx';
import { AntennaIcon } from './Icons.jsx';
import { batteryState, fmtBattery } from './units.js';

const STATUS_TEXT = {
  searching: 'Not connected',
  connecting: 'Connecting…',
  connected: 'Connected',
  no_traffic: 'Connected, no aircraft in range',
  error: 'Error',
};

function OpenPage({ url }) {
  return <LinkButton onClick={() => openUrl(url)}>Open {url.replace(/^http:\/\/|\/$/g, '')}</LinkButton>;
}

// Where the ADSBee's own settings web page is (see adsbee.rs).
function SettingsPageSection({ page }) {
  if (!page) return null;
  return (
    <SheetSection title="ADSBee settings">
      {page.kind === 'url' && (
        <SheetRow label="Settings page">
          <OpenPage url={page.url} />
        </SheetRow>
      )}
      {page.kind === 'hotspot' && (
        <>
          <p className="sheet-empty">
            The ADSBee isn't on your network. Join its Wi-Fi, then open the page. You'll be offline while
            joined.
          </p>
          <SheetRow label="Wi-Fi network">
            <span className="sheet-value mono">{page.ssid}</span>
          </SheetRow>
          <SheetRow label="Password">
            <span className="sheet-value mono">{page.password}</span>
          </SheetRow>
          <SheetRow label="Settings page">
            <OpenPage url={page.url} />
          </SheetRow>
        </>
      )}
      {page.kind === 'off' && (
        <p className="sheet-empty">Wi-Fi and Ethernet are off on the ADSBee, so its settings page isn't available.</p>
      )}
    </SheetSection>
  );
}

export default function ReceiverPanel({ receiver, messages, aircraftCount, onClose }) {
  const stats = receiver.stats ?? {};
  return (
    <Sheet title="Receiver" icon={<AntennaIcon />} onClose={onClose}>
      {receiver.detail && <div className="sheet-error">{receiver.detail}</div>}
      {receiver.status === 'searching' && (
        <p className="sheet-empty">Plug in the PingView receiver or an ADSBee 1090, or switch on a Wi-Fi receiver on the same network (or join its PingView-xxxx network). It's detected automatically.</p>
      )}
      <SheetSection>
        <SheetRow label="Status">
          <span className="sheet-value">{STATUS_TEXT[receiver.status] ?? receiver.status}</span>
        </SheetRow>
        {receiver.model && (
          <SheetRow label="Receiver">
            <span className="sheet-value">{receiver.model}</span>
          </SheetRow>
        )}
        <SheetRow label="Port">
          <span className="sheet-value mono">{receiver.port ?? '—'}</span>
        </SheetRow>
        {receiver.battery && (
          <SheetRow label="Battery">
            <span className={`sheet-value battery-${batteryState(receiver.battery)}`}>
              {fmtBattery(receiver.battery)}
            </span>
          </SheetRow>
        )}
        <SheetRow label="Aircraft">
          <span className="sheet-value">{aircraftCount}</span>
        </SheetRow>
        <SheetRow label="Traffic reports">
          <span className="sheet-value">{receiver.msg_rate.toFixed(1)} /s</span>
        </SheetRow>
      </SheetSection>
      <SettingsPageSection page={receiver.settings_page} />
      <SheetSection title="Diagnostics">
        <SheetRow label="Reports received">
          <span className="sheet-value">{messages}</span>
        </SheetRow>
        <SheetRow label="Frames verified">
          <span className="sheet-value">{stats.verified ?? 0}</span>
        </SheetRow>
        <SheetRow label="Status frames">
          <span className="sheet-value">{stats.other_frames ?? 0}</span>
        </SheetRow>
        <SheetRow label="CRC errors">
          <span className="sheet-value">{stats.crc_errors ?? 0}</span>
        </SheetRow>
      </SheetSection>
    </Sheet>
  );
}
