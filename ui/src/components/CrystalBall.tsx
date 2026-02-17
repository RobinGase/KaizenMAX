import { useEffect, useRef, useState } from "react";
import type { CrystalBallEvent } from "../types";

interface CrystalBallProps {
  events: CrystalBallEvent[];
  onClose: () => void;
}

/**
 * Crystal Ball Feed - Twitch-style draggable/resizable overlay.
 * Shows AI:AI:HUMAN communications backed by Mattermost (Phase E).
 */
function CrystalBall({ events, onClose }: CrystalBallProps) {
  const [position, setPosition] = useState({ x: 24, y: 24 });
  const [size, setSize] = useState({ width: 420, height: 360 });

  const dragRef = useRef<{
    startX: number;
    startY: number;
    originX: number;
    originY: number;
  } | null>(null);
  const resizeRef = useRef<{
    startX: number;
    startY: number;
    originWidth: number;
    originHeight: number;
  } | null>(null);

  useEffect(() => {
    const handleMove = (event: MouseEvent) => {
      if (dragRef.current) {
        const dx = event.clientX - dragRef.current.startX;
        const dy = event.clientY - dragRef.current.startY;
        setPosition({
          x: Math.max(8, dragRef.current.originX + dx),
          y: Math.max(8, dragRef.current.originY + dy),
        });
      }

      if (resizeRef.current) {
        const dx = event.clientX - resizeRef.current.startX;
        const dy = event.clientY - resizeRef.current.startY;
        setSize({
          width: Math.min(640, Math.max(320, resizeRef.current.originWidth + dx)),
          height: Math.min(560, Math.max(220, resizeRef.current.originHeight + dy)),
        });
      }
    };

    const handleUp = () => {
      dragRef.current = null;
      resizeRef.current = null;
    };

    window.addEventListener("mousemove", handleMove);
    window.addEventListener("mouseup", handleUp);
    return () => {
      window.removeEventListener("mousemove", handleMove);
      window.removeEventListener("mouseup", handleUp);
    };
  }, []);

  const formatTime = (value: string) => {
    const seconds = Number(value);
    if (Number.isFinite(seconds)) {
      return new Date(seconds * 1000).toLocaleTimeString();
    }

    const date = new Date(value);
    if (!Number.isNaN(date.getTime())) {
      return date.toLocaleTimeString();
    }

    return value;
  };

  return (
    <div
      className="crystal-ball-overlay"
      style={{
        right: "auto",
        bottom: "auto",
        left: `${position.x}px`,
        top: `${position.y}px`,
        width: `${size.width}px`,
        height: `${size.height}px`,
      }}
    >
      <div
        className="crystal-ball-header"
        onMouseDown={(event) => {
          if ((event.target as HTMLElement).closest("button")) {
            return;
          }
          dragRef.current = {
            startX: event.clientX,
            startY: event.clientY,
            originX: position.x,
            originY: position.y,
          };
        }}
      >
        <h3>Crystal Ball</h3>
        <button onClick={onClose}>Close</button>
      </div>
      <div className="crystal-ball-feed">
        {events.length === 0 ? (
          <p className="feed-empty">
            No events yet. The feed updates from `/api/events`.
          </p>
        ) : (
          events.map((evt) => (
            <div key={evt.eventId} className="feed-event">
              <span className="event-time">{formatTime(evt.timestamp)}</span>
              <span className="event-source">{evt.sourceActor}</span>
              <span className="event-msg">{evt.message}</span>
            </div>
          ))
        )}
      </div>
      <div
        className="crystal-ball-resizer"
        onMouseDown={(event) => {
          event.preventDefault();
          resizeRef.current = {
            startX: event.clientX,
            startY: event.clientY,
            originWidth: size.width,
            originHeight: size.height,
          };
        }}
      />
    </div>
  );
}

export default CrystalBall;
