"use client";

import { useEffect, useId, useRef, useState } from "react";

export type DemoClip = {
  id: string;
  label: string;
  title: string;
  caption: string;
  alt: string;
  src: string;
  srcDark: string;
  video: string;
  videoDark: string;
};

export function DemoGallery({
  heading,
  headingId = "demo-gallery-title",
  clips,
}: {
  heading: string;
  headingId?: string;
  clips: DemoClip[];
}) {
  const baseId = useId();
  const [active, setActive] = useState(0);
  const [paused, setPaused] = useState(false);
  const dayRef = useRef<HTMLVideoElement>(null);
  const nightRef = useRef<HTMLVideoElement>(null);
  const tabRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const clip = clips[active] ?? clips[0];

  useEffect(() => {
    const id = window.location.hash.replace(/^#/, "");
    const index = clips.findIndex((item) => item.id === id);
    if (index >= 0) {
      setActive(index);
    }
  }, [clips]);

  useEffect(() => {
    setPaused(false);
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    for (const node of [dayRef.current, nightRef.current]) {
      if (!node) {
        continue;
      }
      node.currentTime = 0;
      if (reducedMotion) {
        node.pause();
      } else {
        void node.play();
      }
    }
  }, [clip?.id]);

  if (!clip) {
    return null;
  }

  const select = (index: number) => {
    const next = clips[index];
    if (!next) {
      return;
    }
    setActive(index);
    const url = new URL(window.location.href);
    url.hash = next.id;
    window.history.replaceState(null, "", url);
  };

  const move = (from: number, delta: number) => {
    const next = (from + delta + clips.length) % clips.length;
    select(next);
    tabRefs.current[next]?.focus();
  };

  const togglePlayback = () => {
    const nextPaused = !paused;
    setPaused(nextPaused);
    for (const node of [dayRef.current, nightRef.current]) {
      if (!node) {
        continue;
      }
      if (nextPaused) {
        node.pause();
      } else {
        void node.play();
      }
    }
  };

  const panelId = `${baseId}-panel`;
  const label = clip.alt;

  return (
    <section className="gardn-section gardn-demo" aria-labelledby={headingId}>
      <h2 id={headingId} className="gardn-section-title">
        {heading}
      </h2>
      <div
        className="gardn-demo-tabs"
        role="tablist"
        aria-labelledby={headingId}
        onKeyDown={(event) => {
          if (event.key === "ArrowRight") {
            event.preventDefault();
            move(active, 1);
          } else if (event.key === "ArrowLeft") {
            event.preventDefault();
            move(active, -1);
          } else if (event.key === "Home") {
            event.preventDefault();
            select(0);
            tabRefs.current[0]?.focus();
          } else if (event.key === "End") {
            event.preventDefault();
            select(clips.length - 1);
            tabRefs.current[clips.length - 1]?.focus();
          }
        }}
      >
        {clips.map((item, index) => {
          const selected = index === active;
          const tabId = `${baseId}-tab-${item.id}`;
          return (
            <button
              key={item.id}
              ref={(node) => {
                tabRefs.current[index] = node;
              }}
              type="button"
              className="gardn-demo-tab"
              role="tab"
              id={tabId}
              aria-selected={selected}
              aria-controls={panelId}
              tabIndex={selected ? 0 : -1}
              onClick={() => select(index)}
            >
              {item.label}
            </button>
          );
        })}
      </div>
      <div
        className="gardn-demo-panel"
        role="tabpanel"
        id={panelId}
        aria-labelledby={`${baseId}-tab-${clip.id}`}
      >
        <figure className="gardn-session-shot gardn-demo-stage">
          <img
            className="gardn-session-still gardn-session-day"
            src={clip.src}
            width={1440}
            height={912}
            alt={label}
          />
          <img
            className="gardn-session-still gardn-session-night"
            src={clip.srcDark}
            width={1440}
            height={912}
            alt={label}
          />
          <video
            key={`${clip.id}-day`}
            ref={dayRef}
            className="gardn-session-motion gardn-session-day"
            width={1440}
            height={912}
            poster={clip.src}
            muted
            loop
            playsInline
            aria-label={label}
          >
            <source src={clip.video} type="video/mp4" />
          </video>
          <video
            key={`${clip.id}-night`}
            ref={nightRef}
            className="gardn-session-motion gardn-session-night"
            width={1440}
            height={912}
            poster={clip.srcDark}
            muted
            loop
            playsInline
            aria-label={label}
          >
            <source src={clip.videoDark} type="video/mp4" />
          </video>
          <figcaption className="gardn-demo-caption">
            <strong className="gardn-demo-caption-title">{clip.title}</strong>
            <span>{clip.caption}</span>
            <button
              type="button"
              className="gardn-demo-pause"
              onClick={togglePlayback}
              aria-pressed={paused}
            >
              {paused ? "Play video" : "Pause video"}
            </button>
          </figcaption>
        </figure>
      </div>
    </section>
  );
}
