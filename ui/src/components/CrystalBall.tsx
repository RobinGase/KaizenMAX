import { useState } from "react";
import type { CrystalBallEvent } from "../types";

interface CrystalBallProps {
  onClose: () => void;
}

/**
 * Crystal Ball Feed - Twitch-style draggable/resizable overlay.
 * Shows AI:AI:HUMAN communications backed by Mattermost (Phase E).
 */
function CrystalBall({ onClose }: CrystalBallProps) {
  const [events] = useState<CrystalBallEvent[]>([]);

  return (
    <div className="crystal-ball-overlay">
      <div className="crystal-ball-header">
        <h3>Crystal Ball</h3>
        <button onClick={onClose}>X</button>
      </div>
      <div className="crystal-ball-feed">
        {events.length === 0 ? (
          <p className="feed-empty">
            No events yet. Feed connects to Mattermost in Phase E.
          </p>
        ) : (
          events.map((evt) => (
            <div key={evt.eventId} className="feed-event">
              <span className="event-time">
                {new Date(evt.timestamp).toLocaleTimeString()}
              </span>
              <span className="event-source">{evt.sourceActor}</span>
              <span className="event-msg">{evt.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}

export default CrystalBall;
