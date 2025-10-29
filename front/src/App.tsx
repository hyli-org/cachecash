import { useState, useEffect, useCallback, useRef, ChangeEvent, FormEvent } from "react";
import "./App.css";
import { nodeService } from "./services/NodeService";
import { deriveKeyPairFromName, DerivedKeyPair } from "./services/KeyService";

import { TransactionList } from "./components/TransactionList";
import { DebugNotesPanel } from "./components/DebugNotesPanel";
import { ManageNotesModal } from "./components/ManageNotesModal";
import slice1 from "./audio/slice1.mp3";
import slice2 from "./audio/slice2.mp3";
import slice3 from "./audio/slice3.mp3";
import bombSound from "./audio/bomb.mp3";
import { declareCustomElement } from "testnet-maintenance-widget";
import { useStoredNotes } from "./hooks/useStoredNotes";
import { useDebugMode } from "./hooks/useDebugMode";
import { addStoredNote } from "./services/noteStorage";
import { useDebounce } from "use-debounce";
import { StoredNote } from "./types/note";
declareCustomElement();

// Mutex implementation
class Mutex {
  private locked: boolean = false;
  private queue: Array<() => void> = [];

  async acquire(): Promise<void> {
    if (!this.locked) {
      this.locked = true;
      return Promise.resolve();
    }

    return new Promise<void>((resolve) => {
      this.queue.push(resolve);
    });
  }

  release(): void {
    if (this.queue.length > 0) {
      const next = this.queue.shift();
      if (next) next();
    } else {
      this.locked = false;
    }
  }
}

// Add global mutexes
declare global {
  interface Window {
    orangeMutex: Mutex;
    bombMutex: Mutex;
    slicedOranges: Set<number>;
    slicedBombs: Set<number>;
  }
}

// Initialize global mutexes
if (!window.orangeMutex) {
  window.orangeMutex = new Mutex();
}
if (!window.bombMutex) {
  window.bombMutex = new Mutex();
}
if (!window.slicedOranges) {
  window.slicedOranges = new Set();
}
if (!window.slicedBombs) {
  window.slicedBombs = new Set();
}

interface Orange {
  id: number;
  x: number;
  y: number;
  rotation: number;
  speed: number;
  sliced: boolean;
}

interface Bomb {
  id: number;
  x: number;
  y: number;
  rotation: number;
  speed: number;
  sliced: boolean;
}

interface JuiceParticle {
  id: number;
  x: number;
  y: number;
  velocityX: number;
  velocityY: number;
  time: number;
}

interface ExplosionParticle {
  id: number;
  x: number;
  y: number;
  velocityX: number;
  velocityY: number;
  size: number;
  color: string;
  time: number;
}

interface ScorePopup {
  id: number;
  x: number;
  y: number;
  text: string;
  variant: "positive" | "negative";
}

interface TransactionEntry {
  title: string;
  hash?: string;
  timestamp: number;
}

const SPAWN_INTERVAL = 500;
const GRAVITY = 0.01;
const INITIAL_SPEED = 1;
const BASE_ORANGE_SIZE = 200;
const BASE_BOMB_SIZE = 200;
const MOBILE_BREAKPOINT = 768;
const SMALL_MOBILE_BREAKPOINT = 480;
const MAX_PENALTY_DISPLAY = 30;
const SLICE_SPEED_WINDOW_MS = 8000; // Rolling window for slices per second
const MAX_SLICE_SPEED = 5; // Upper bound for gauge visualization
const SECRET_VIDEO_POOL: string[] = ["/videos/clip1.mp4", "/videos/clip2.mp4", "/videos/clip3.mp4"];

interface ViewportSpriteSizes {
  orangeSize: number;
  bombSize: number;
}

const computeSpriteSizes = (width: number): ViewportSpriteSizes => {
  if (width <= SMALL_MOBILE_BREAKPOINT) {
    return { orangeSize: 140, bombSize: 160 };
  }

  if (width <= MOBILE_BREAKPOINT) {
    return { orangeSize: 160, bombSize: 180 };
  }

  return { orangeSize: BASE_ORANGE_SIZE, bombSize: BASE_BOMB_SIZE };
};

const deriveNoteReference = (note: unknown): string | undefined => {
  if (!note || typeof note !== "object") {
    return undefined;
  }

  const candidates = [
    (note as { psi?: string }).psi,
    (note as { address?: string }).address,
    (note as { contract?: string }).contract,
    (note as { value?: string }).value,
    (note as { kind?: string }).kind,
  ];

  return candidates.find((candidate): candidate is string => typeof candidate === "string" && candidate.length > 0);
};

function App() {
  const debugMode = useDebugMode();
  const [playerName, setPlayerName] = useState(() => localStorage.getItem("playerName") || "");
  const { notes: storedNotes, clearNotes } = useStoredNotes(playerName);
  const [isManageModalOpen, setIsManageModalOpen] = useState(false);
  const [nameInput, setNameInput] = useState(() => localStorage.getItem("playerName") || "");
  const [playerKeys, setPlayerKeys] = useState<DerivedKeyPair | null>(() => {
    const storedPlayer = localStorage.getItem("playerName");
    if (!storedPlayer) {
      return null;
    }

    const storedPrivate = localStorage.getItem(`keys:${storedPlayer}:private`);
    const storedPublic = localStorage.getItem(`keys:${storedPlayer}:public`);

    if (
      storedPrivate &&
      storedPublic &&
      storedPrivate.length === 64 &&
      storedPublic.length === 64
    ) {
      return { privateKey: storedPrivate, publicKey: storedPublic };
    }

    try {
      return deriveKeyPairFromName(storedPlayer);
    } catch (error) {
      console.warn("Failed to derive key pair from stored player name", error);
      return null;
    }
  });
  const [oranges, setOranges] = useState<Orange[]>([]);
  const [bombs, setBombs] = useState<Bomb[]>([]);
  const [bombPenalty, setBombPenalty] = useState(() => {
    if (!playerName) {
      return 0;
    }
    const stored = localStorage.getItem(`bombPenalty:${playerName}`);
    return stored ? Number(stored) || 0 : 0;
  });
  const [isScoreShaking, setIsScoreShaking] = useState(false);
  const gameAreaRef = useRef<HTMLDivElement>(null);
  const nextOrangeId = useRef(0);
  const lastMousePosition = useRef({ x: 0, y: 0 });
  const isMouseDown = useRef(false);
  const slicePoints = useRef<{ x: number; y: number }[]>([]);
  const sliceStartTime = useRef<number>(0);
  const lastSpawnTimeRef = useRef(performance.now());
  const [juiceParticles, setJuiceParticles] = useState<JuiceParticle[]>([]);
  const nextJuiceId = useRef(0);
  const [explosionParticles, setExplosionParticles] = useState<ExplosionParticle[]>([]);
  const nextExplosionId = useRef(0);
  const [scorePopups, setScorePopups] = useState<ScorePopup[]>([]);
  const nextScorePopupId = useRef(0);
  const [transactions, setTransactions] = useState<TransactionEntry[]>([]);
  const [spriteSizes, setSpriteSizes] = useState<ViewportSpriteSizes>(() => {
    if (typeof window === "undefined") {
      return { orangeSize: BASE_ORANGE_SIZE, bombSize: BASE_BOMB_SIZE };
    }
    return computeSpriteSizes(window.innerWidth);
  });
  const [isMobileLayout, setIsMobileLayout] = useState(() =>
    typeof window !== "undefined" ? window.innerWidth <= SMALL_MOBILE_BREAKPOINT : false,
  );
  const [isScoreboardCollapsed, setIsScoreboardCollapsed] = useState(() =>
    typeof window !== "undefined" ? window.innerWidth <= SMALL_MOBILE_BREAKPOINT : false,
  );
  const previousScoreboardCollapsedRef = useRef(isScoreboardCollapsed);
  const [isSecretVideoOpen, setIsSecretVideoOpen] = useState(false);
  const [secretVideoSources, setSecretVideoSources] = useState<string[]>([]);

  useEffect(() => {
    const handleResize = () => {
      setSpriteSizes((prev) => {
        const next = computeSpriteSizes(window.innerWidth);
        if (next.orangeSize === prev.orangeSize && next.bombSize === prev.bombSize) {
          return prev;
        }
        return next;
      });
      setIsMobileLayout(window.innerWidth <= SMALL_MOBILE_BREAKPOINT);
    };

    window.addEventListener("resize", handleResize);
    return () => window.removeEventListener("resize", handleResize);
  }, []);

  const selectSecretVideos = useCallback(() => {
    if (SECRET_VIDEO_POOL.length === 0) {
      setSecretVideoSources([]);
      return;
    }
    const shuffled = [...SECRET_VIDEO_POOL].sort(() => Math.random() - 0.5);
    setSecretVideoSources(shuffled.slice(0, 2));
  }, []);

  useEffect(() => {
    selectSecretVideos();
  }, [selectSecretVideos]);

  useEffect(() => {
    if (isMobileLayout) {
      setIsScoreboardCollapsed(true);
    } else {
      setIsScoreboardCollapsed(false);
    }
  }, [isMobileLayout]);

  const orangeSize = spriteSizes.orangeSize;
  const bombSize = spriteSizes.bombSize;
  const orangeSliceThreshold = orangeSize / 2;
  const bombSliceThreshold = bombSize / 2;
  const offscreenBuffer = Math.max(orangeSize, 200);
  const [submissionError, setSubmissionError] = useState<string | null>(null);
  const noteBalance = storedNotes.length;
  const sliceTimestampsRef = useRef<number[]>([]);
  const [rawSliceSpeed, setRawSliceSpeed] = useState(0);
  const [debouncedSliceSpeed] = useDebounce(rawSliceSpeed, 200);
  const penaltyMeterPercent = Math.min(bombPenalty / MAX_PENALTY_DISPLAY, 1);
  const penaltyDisplayText = bombPenalty > 0 ? `${bombPenalty} pumpkins` : "None";

  useEffect(() => {
    if (!playerName) {
      setIsManageModalOpen(false);
      sliceTimestampsRef.current = [];
      setRawSliceSpeed(0);
    }
  }, [playerName]);

  const updateSliceSpeed = useCallback(() => {
    const now = Date.now();
    const cutoff = now - SLICE_SPEED_WINDOW_MS;
    const filtered = sliceTimestampsRef.current.filter((timestamp) => timestamp >= cutoff);
    sliceTimestampsRef.current = filtered;
    if (filtered.length === 0) {
      setRawSliceSpeed(0);
      return;
    }

    const earliest = filtered[0];
    const elapsedMs = Math.max(now - earliest, 1);
    const rate = filtered.length / (elapsedMs / 1000);
    setRawSliceSpeed(rate);
  }, []);

  useEffect(() => {
    if (!playerName) {
      setRawSliceSpeed(0);
      sliceTimestampsRef.current = [];
      return;
    }

    updateSliceSpeed();
    const interval = window.setInterval(updateSliceSpeed, 400);
    return () => window.clearInterval(interval);
  }, [playerName, updateSliceSpeed]);

  const handleNameChange = useCallback((event: ChangeEvent<HTMLInputElement>) => {
    setNameInput(event.target.value);
  }, []);

  const handleNameSubmit = useCallback(
    (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      const trimmed = nameInput.trim();
      if (!trimmed) {
        setPlayerName("");
        return;
      }
      setPlayerName(trimmed);
      setSubmissionError(null);
    },
    [nameInput, setPlayerName, setSubmissionError],
  );

  const handleLogout = useCallback(() => {
    setPlayerName("");
    setSubmissionError(null);
  }, [setPlayerName, setSubmissionError]);

  const handleOpenManageModal = useCallback(() => {
    if (!playerName) {
      return;
    }
    if (isMobileLayout) {
      previousScoreboardCollapsedRef.current = isScoreboardCollapsed;
      setIsScoreboardCollapsed(true);
    }
    setIsManageModalOpen(true);
  }, [playerName, isMobileLayout, isScoreboardCollapsed]);

  const handleCloseManageModal = useCallback(() => {
    setIsManageModalOpen(false);
    if (isMobileLayout) {
      setIsScoreboardCollapsed(previousScoreboardCollapsedRef.current);
    }
  }, [isMobileLayout]);

  const handleToggleSecretVideo = useCallback(() => {
    if (isMobileLayout || SECRET_VIDEO_POOL.length === 0) {
      return;
    }
    if (!isSecretVideoOpen) {
      selectSecretVideos();
    }
    setIsSecretVideoOpen((prev) => !prev);
  }, [isMobileLayout, isSecretVideoOpen, selectSecretVideos]);

  const handleToggleScoreboard = useCallback(() => {
    setIsScoreboardCollapsed((prev) => !prev);
  }, []);


  const createJuiceEffect = useCallback((x: number, y: number) => {
    const particles: JuiceParticle[] = [];
    const particleCount = 12; // Nombre de particules de jus
    const initialSpeed = 3; // Vitesse initiale

    for (let i = 0; i < particleCount; i++) {
      const angle = (i * 360) / particleCount + Math.random() * 30 - 15; // Angle avec un peu de variation
      const speed = initialSpeed + Math.random() * 5; // Plus de variation dans la vitesse
      const radian = (angle * Math.PI) / 180;

      // Calcul des composantes de la vitesse initiale
      const velocityX = Math.cos(radian) * speed;
      const velocityY = Math.sin(radian) * speed;

      particles.push({
        id: nextJuiceId.current++,
        x,
        y,
        velocityX,
        velocityY,
        time: 0,
      });
    }

    setJuiceParticles((prev) => [...prev, ...particles]);

    // Nettoyer les particules après l'animation
    setTimeout(() => {
      setJuiceParticles((prev) => prev.filter((p) => !particles.some((newP) => newP.id === p.id)));
    }, 1500);
  }, []);

  const createExplosionEffect = useCallback((x: number, y: number) => {
    const particles: ExplosionParticle[] = [];
    const particleCount = 20;
    const colors = ["#ff4444", "#ff8800", "#ffcc00", "#ff0000"];

    for (let i = 0; i < particleCount; i++) {
      const angle = Math.random() * Math.PI * 2;
      const speed = 2 + Math.random() * 4;
      const size = 3 + Math.random() * 5;
      const color = colors[Math.floor(Math.random() * colors.length)];

      particles.push({
        id: nextExplosionId.current++,
        x,
        y,
        velocityX: Math.cos(angle) * speed,
        velocityY: Math.sin(angle) * speed,
        size,
        color,
        time: 0,
      });
    }

    setExplosionParticles((prev) => [...prev, ...particles]);

    setTimeout(() => {
      setExplosionParticles((prev) => prev.filter((p) => !particles.some((newP) => newP.id === p.id)));
    }, 1000);
  }, []);

  const addScorePopup = useCallback((x: number, y: number, text: string, variant: "positive" | "negative") => {
    const id = nextScorePopupId.current++;
    const popup: ScorePopup = {
      id,
      x,
      y,
      text,
      variant,
    };

    setScorePopups((prev) => [...prev, popup]);

    setTimeout(() => {
      setScorePopups((prev) => prev.filter((existing) => existing.id !== id));
    }, 800);
  }, []);

  const sliceBomb = async (bombId: number) => {
    if (!playerName) return;
    try {
      await window.bombMutex.acquire();
      const bomb = bombs.find((b) => b.id === bombId);
      if (!bomb || bomb.sliced || window.slicedBombs.has(bombId)) return;

      // Vibrate for 200ms when slicing a bomb (longer vibration for bombs)
      if ("vibrate" in navigator) {
        navigator.vibrate([1000]);
      }

      // Play bomb sound
      const bombAudio = new Audio(bombSound);
      bombAudio.volume = Math.max(0, Math.min(1, bombAudio.volume * 0.84));
      bombAudio.play();

      // Create explosion effect instead of juice effect
      createExplosionEffect(bomb.x, bomb.y);

      addScorePopup(bomb.x, bomb.y, "-10", "negative");

      // Apply cumulative penalty
      setBombPenalty((prev) => prev + 10);

      // Trigger score shake animation
      setIsScoreShaking(true);
      setTimeout(() => setIsScoreShaking(false), 500);

      setBombs((prev) => prev.map((b) => (b.id === bombId ? { ...b, sliced: true } : b)));

      window.slicedBombs.add(bombId);
    } finally {
      window.bombMutex.release();
    }
  };

  // Submit the slice server-side without blocking future slices
  const submitPumpkinSlice = useCallback(
    async (publicKeyHex: string, playerLabel: string) => {
      try {
        if (!publicKeyHex) {
          throw new Error("Missing player public key");
        }

        const trimmedPlayerName = playerLabel.trim();
        if (!trimmedPlayerName) {
          throw new Error("Player name must not be empty");
        }

        const response = await nodeService.requestFaucet(publicKeyHex);
        const { tx_hash: txHash, note } = response;
        setSubmissionError(null);
        const reference = txHash ?? deriveNoteReference(note);
        if (reference) {
          const stored: StoredNote = {
            txHash: reference,
            note: note ?? response,
            storedAt: Date.now(),
            player: trimmedPlayerName,
          };
          addStoredNote(trimmedPlayerName, stored);
          const now = Date.now();
          sliceTimestampsRef.current = [...sliceTimestampsRef.current, now].filter(
            (timestamp) => timestamp >= now - SLICE_SPEED_WINDOW_MS,
          );
          updateSliceSpeed();
        }
        const shortHash = reference && reference.length > 12 ? `${reference.slice(0, 6)}…${reference.slice(-4)}` : reference;
        const title = `+1 pumpkin${trimmedPlayerName ? ` for ${trimmedPlayerName}` : ""}`;
        setTransactions((prev) =>
          [
            {
              title,
              hash: shortHash || undefined,
              timestamp: Date.now(),
            },
            ...prev,
          ].slice(0, 10),
        );
      } catch (error) {
        console.error("Failed to record slice", error);
        setSubmissionError("We could not reach the server. Your score has not been updated.");
      }
  },
    [setSubmissionError, setTransactions, updateSliceSpeed],
  );

  const sliceOrange = async (orangeId: number) => {
    const keysSnapshot = playerKeys;
    const nameSnapshot = playerName;
    if (!nameSnapshot || !keysSnapshot) return;

    const penaltySnapshot = bombPenalty;
    let submissionPayload: { publicKey: string; playerLabel: string } | null = null;

    await window.orangeMutex.acquire();
    try {
      const orange = oranges.find((o) => o.id === orangeId);
      if (!orange || orange.sliced || window.slicedOranges.has(orangeId)) {
        return;
      }

      if ("vibrate" in navigator) {
        navigator.vibrate(150);
      }

      const sliceSound = [slice1, slice2, slice3];
      const audio = new Audio(sliceSound[Math.floor(Math.random() * sliceSound.length)]);
      audio.volume = Math.max(0, Math.min(1, audio.volume * 0.84));
      audio.currentTime = 0;
      audio.play();

      createJuiceEffect(orange.x, orange.y);
      addScorePopup(orange.x, orange.y, "+1", "positive");

      setOranges((prev) => prev.map((o) => (o.id === orangeId ? { ...o, sliced: true } : o)));
      window.slicedOranges.add(orangeId);

      if (penaltySnapshot === 0) {
        if (keysSnapshot.publicKey && nameSnapshot.trim()) {
          submissionPayload = {
            publicKey: keysSnapshot.publicKey,
            playerLabel: nameSnapshot.trim(),
          };
        } else {
          console.warn("Missing player data for faucet submission");
          setSubmissionError("We could not reach the server. Your score has not been updated.");
        }
      } else {
        setBombPenalty((prev) => (prev > 0 ? prev - 1 : 0));
      }
    } finally {
      window.orangeMutex.release();
    }

    if (submissionPayload) {
      void submitPumpkinSlice(submissionPayload.publicKey, submissionPayload.playerLabel);
    }
  };

  const createSliceEffect = useCallback((points: { x: number; y: number }[]) => {
    if (!gameAreaRef.current || points.length < 2) return;

    const slice = document.createElement("div");
    slice.className = "slice-effect";

    // Créer un SVG pour la ligne
    const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("width", "100%");
    svg.setAttribute("height", "100%");
    svg.style.position = "absolute";
    svg.style.top = "0";
    svg.style.left = "0";

    // Créer le chemin
    const path = document.createElementNS("http://www.w3.org/2000/svg", "path");
    const d = points.reduce((acc, point, i) => {
      return acc + (i === 0 ? `M ${point.x} ${point.y}` : ` L ${point.x} ${point.y}`);
    }, "");
    path.setAttribute("d", d);
    path.setAttribute("stroke", "white");
    path.setAttribute("stroke-width", "2");
    path.setAttribute("fill", "none");
    path.style.filter = "drop-shadow(0 0 2px rgba(255,255,255,0.8))";

    svg.appendChild(path);
    slice.appendChild(svg);
    gameAreaRef.current.appendChild(slice);

    setTimeout(() => slice.remove(), 300);
  }, []);

  const checkSlice = useCallback(
    (startX: number, startY: number, endX: number, endY: number) => {
      const dx = endX - startX;
      const dy = endY - startY;

      // Create slice effect
      createSliceEffect([
        { x: startX, y: startY },
        { x: endX, y: endY },
      ]);

      // Check for oranges and bombs in the slice path
      setOranges((prev) =>
        prev.map((orange) => {
          if (orange.sliced) return orange;

          // Calculate distance from orange to line segment
          const lineLength = Math.sqrt(dx * dx + dy * dy);
          if (lineLength === 0) return orange;

          // Calculate projection of orange position onto the line
          const t = Math.max(
            0,
            Math.min(1, ((orange.x - startX) * dx + (orange.y - startY) * dy) / (lineLength * lineLength)),
          );

          // Calculate closest point on the line segment
          const closestX = startX + t * dx;
          const closestY = startY + t * dy;

          // Calculate actual distance from orange to closest point
          const distance = Math.sqrt(Math.pow(orange.x - closestX, 2) + Math.pow(orange.y - closestY, 2));

          // If orange is close enough to the slice line
          if (distance < orangeSliceThreshold) {
            sliceOrange(orange.id);
            return orange;
          }
          return orange;
        }),
      );

      // Check for bombs
      setBombs((prev) =>
        prev.map((bomb) => {
          if (bomb.sliced) return bomb;

          const lineLength = Math.sqrt(dx * dx + dy * dy);
          if (lineLength === 0) return bomb;

          const t = Math.max(
            0,
            Math.min(1, ((bomb.x - startX) * dx + (bomb.y - startY) * dy) / (lineLength * lineLength)),
          );

          const closestX = startX + t * dx;
          const closestY = startY + t * dy;

          const distance = Math.sqrt(Math.pow(bomb.x - closestX, 2) + Math.pow(bomb.y - closestY, 2));

          if (distance < bombSliceThreshold) {
            sliceBomb(bomb.id);
            return bomb;
          }
          return bomb;
        }),
      );
    },
    [createSliceEffect, sliceOrange, sliceBomb, orangeSliceThreshold, bombSliceThreshold],
  );

  const handleMouseDown = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    if (!gameAreaRef.current) return;
    const rect = gameAreaRef.current.getBoundingClientRect();
    isMouseDown.current = true;
    sliceStartTime.current = Date.now();
    const position = {
      x: event.clientX - rect.left,
      y: event.clientY - rect.top,
    };
    lastMousePosition.current = position;
    slicePoints.current = [position];
  }, []);

  const handleTouchStart = useCallback((event: React.TouchEvent<HTMLDivElement>) => {
    if (!gameAreaRef.current) return;
    event.preventDefault(); // Prevent scrolling while slicing
    const rect = gameAreaRef.current.getBoundingClientRect();
    isMouseDown.current = true;
    sliceStartTime.current = Date.now();
    const touch = event.touches[0];
    const position = {
      x: touch.clientX - rect.left,
      y: touch.clientY - rect.top,
    };
    lastMousePosition.current = position;
    slicePoints.current = [position];
  }, []);

  const handleMouseMove = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      if (!isMouseDown.current || !gameAreaRef.current) return;

      // Check if slice duration exceeds 200ms
      if (Date.now() - sliceStartTime.current > 200) {
        isMouseDown.current = false;
        createSliceEffect(slicePoints.current);
        slicePoints.current = [];
        return;
      }

      const rect = gameAreaRef.current.getBoundingClientRect();
      const currentX = event.clientX - rect.left;
      const currentY = event.clientY - rect.top;

      // Ajouter le point au chemin
      slicePoints.current.push({ x: currentX, y: currentY });

      // Vérifier les oranges sur le chemin
      const dx = currentX - lastMousePosition.current.x;
      const dy = currentY - lastMousePosition.current.y;
      const distance = Math.sqrt(dx * dx + dy * dy);

      if (distance > 10) {
        checkSlice(lastMousePosition.current.x, lastMousePosition.current.y, currentX, currentY);
        lastMousePosition.current = { x: currentX, y: currentY };
      }
    },
    [checkSlice, createSliceEffect],
  );

  const handleTouchMove = useCallback(
    (event: React.TouchEvent<HTMLDivElement>) => {
      if (!isMouseDown.current || !gameAreaRef.current) return;
      event.preventDefault(); // Prevent scrolling while slicing

      // Check if slice duration exceeds 200ms
      if (Date.now() - sliceStartTime.current > 200) {
        isMouseDown.current = false;
        createSliceEffect(slicePoints.current);
        slicePoints.current = [];
        return;
      }

      const rect = gameAreaRef.current.getBoundingClientRect();
      const touch = event.touches[0];
      const currentX = touch.clientX - rect.left;
      const currentY = touch.clientY - rect.top;

      // Ajouter le point au chemin
      slicePoints.current.push({ x: currentX, y: currentY });

      // Vérifier les oranges sur le chemin
      const dx = currentX - lastMousePosition.current.x;
      const dy = currentY - lastMousePosition.current.y;
      const distance = Math.sqrt(dx * dx + dy * dy);

      if (distance > 10) {
        checkSlice(lastMousePosition.current.x, lastMousePosition.current.y, currentX, currentY);
        lastMousePosition.current = { x: currentX, y: currentY };
      }
    },
    [checkSlice, createSliceEffect],
  );

  const handleMouseUp = useCallback(() => {
    if (isMouseDown.current) {
      createSliceEffect(slicePoints.current);
      slicePoints.current = [];
    }
    isMouseDown.current = false;
  }, [createSliceEffect]);

  const handleTouchEnd = useCallback(() => {
    if (isMouseDown.current) {
      createSliceEffect(slicePoints.current);
      slicePoints.current = [];
    }
    isMouseDown.current = false;
  }, [createSliceEffect]);

  const spawnOrange = useCallback(
    (currentTime: number) => {
      if (!gameAreaRef.current) return;

      const gameArea = gameAreaRef.current;

      const timeSinceLastSpawn = currentTime - lastSpawnTimeRef.current;
      const clampedWidth = (Math.max(Math.min(gameArea.clientWidth, 1800), 400) - 400) / 1400;
      const widthMult = 1.2 - 0.6 * clampedWidth;
      if (timeSinceLastSpawn < SPAWN_INTERVAL * widthMult) {
        return; // Skip spawning if not enough time has passed
      }
      lastSpawnTimeRef.current = currentTime + Math.random() * SPAWN_INTERVAL * 0.4 - SPAWN_INTERVAL * 0.2;

      const gameWidth = gameArea.clientWidth;
      const computeSpawnX = (size: number) => {
        const radius = size / 2;
        if (gameWidth <= size) {
          return gameWidth / 2;
        }
        const min = radius;
        const max = gameWidth - radius;
        return min + Math.random() * (max - min);
      };

      // 20% chance to spawn a bomb instead of an orange
      if (Math.random() < 0.2) {
        const bomb: Bomb = {
          id: nextOrangeId.current++,
          x: computeSpawnX(bombSize),
          y: -bombSize,
          rotation: Math.random() * 360,
          speed: INITIAL_SPEED,
          sliced: false,
        };
        setBombs((prev) => [...prev, bomb]);
      } else {
        const orange: Orange = {
          id: nextOrangeId.current++,
          x: computeSpawnX(orangeSize),
          y: -orangeSize,
          rotation: Math.random() * 360,
          speed: INITIAL_SPEED,
          sliced: false,
        };
        setOranges((prev) => [...prev, orange]);
      }
    },
    [bombSize, orangeSize],
  );

  useEffect(() => {
    if (!playerName) {
      localStorage.removeItem("playerName");
      return;
    }

    localStorage.setItem("playerName", playerName);
  }, [playerName]);

  useEffect(() => {
    if (!playerName) {
      setPlayerKeys(null);
      return;
    }

    try {
      const derivedKeys = deriveKeyPairFromName(playerName);
      setPlayerKeys(derivedKeys);
    } catch (error) {
      console.error("Failed to derive key pair", error);
      setPlayerKeys(null);
    }
  }, [playerName]);

  useEffect(() => {
    if (!playerName || !playerKeys) {
      return;
    }

    localStorage.setItem(`keys:${playerName}:public`, playerKeys.publicKey);
    localStorage.setItem(`keys:${playerName}:private`, playerKeys.privateKey);
  }, [playerName, playerKeys]);

  useEffect(() => {
    if (!playerName) {
      setBombPenalty(0);
      setNameInput("");
      return;
    }

    const storedPenalty = localStorage.getItem(`bombPenalty:${playerName}`);
    setBombPenalty(storedPenalty ? Number(storedPenalty) || 0 : 0);

    localStorage.removeItem(`count:${playerName}`);

    setNameInput(playerName);
  }, [playerName]);

  // Save state to localStorage
  useEffect(() => {
    if (!playerName) {
      return;
    }

    localStorage.setItem(`bombPenalty:${playerName}`, bombPenalty.toString());
    localStorage.removeItem(`achievements:${playerName}`);
  }, [playerName, bombPenalty]);

  // Update orange and bomb positions
  useEffect(() => {
    let currentTime = performance.now();
    let animationFrameId = requestAnimationFrame(function animate(time) {
      const elapsed = time - currentTime;
      currentTime = time;

      if (!document.hidden) {
        spawnOrange(time);
      }

      setOranges((prev) =>
        prev
          .map((orange) => ({
            ...orange,
            y: orange.y + orange.speed * (elapsed / 10),
            speed: orange.speed + GRAVITY * (elapsed / 10),
            rotation: orange.rotation + 2 * (elapsed / 10),
          }))
          .filter((orange) => orange.y < window.innerHeight + offscreenBuffer),
      );

      setBombs((prev) =>
        prev
          .map((bomb) => ({
            ...bomb,
            y: bomb.y + bomb.speed * (elapsed / 10),
            speed: bomb.speed + GRAVITY * (elapsed / 10),
            rotation: bomb.rotation + 2 * (elapsed / 10),
          }))
          .filter((bomb) => bomb.y < window.innerHeight + Math.max(bombSize * 2, 200)),
      );

      animationFrameId = requestAnimationFrame(animate);
    });

    return () => cancelAnimationFrame(animationFrameId);
  }, [spawnOrange, offscreenBuffer, bombSize]);

  // Mettre à jour la position des particules avec la balistique
  useEffect(() => {
    const animationFrame = requestAnimationFrame(function animate() {
      setJuiceParticles((prev) =>
        prev.map((particle) => {
          const time = particle.time + 0.016; // ~60fps
          // Mise à jour de la vitesse verticale avec la gravité (augmentée)
          const currentVelocityY = particle.velocityY + GRAVITY * 3;

          // Mise à jour de la position
          const newX = particle.x + particle.velocityX;
          const newY = particle.y + currentVelocityY;

          return {
            ...particle,
            x: newX,
            y: newY,
            velocityY: currentVelocityY,
            time,
          };
        }),
      );
      requestAnimationFrame(animate);
    });

    return () => cancelAnimationFrame(animationFrame);
  }, []);

  // Update explosion particles
  useEffect(() => {
    const animationFrame = requestAnimationFrame(function animate() {
      setExplosionParticles((prev) =>
        prev.map((particle) => {
          const time = particle.time + 0.016;
          const currentVelocityY = particle.velocityY + GRAVITY * 2;

          return {
            ...particle,
            x: particle.x + particle.velocityX,
            y: particle.y + currentVelocityY,
            velocityY: currentVelocityY,
            time,
          };
        }),
      );
      requestAnimationFrame(animate);
    });

    return () => cancelAnimationFrame(animationFrame);
  }, []);

  const appClassName = `App${bombPenalty > 0 ? " App--penalty" : ""}${
    isMobileLayout && isScoreboardCollapsed ? " App--scoreboard-collapsed" : ""
  }${isManageModalOpen ? " App--modal-open" : ""}`;
  const titleBadgeClassName = `pumpkin-title__badge${bombPenalty > 0 ? " pumpkin-title__badge--warning" : ""}`;
  const gameAreaClassName = "game-area";

  return (
    <div className={appClassName}>
      <TransactionList
        transactions={transactions}
        setTransactions={setTransactions}
        isMobile={isMobileLayout}
        isSecretVideoOpen={isSecretVideoOpen}
      />

      <div className="pumpkin-title">
        <button type="button" className="pumpkin-title__badge-button" onClick={handleToggleSecretVideo}>
          <span className={titleBadgeClassName} aria-hidden="true">
            Cache Cash
          </span>
        </button>
      </div>
      <div className="game-layout">
        {!isMobileLayout && isSecretVideoOpen && secretVideoSources[0] && (
          <aside className="secret-video-panel secret-video-panel--left" aria-label="Secret Video">
            <video src={secretVideoSources[0]} playsInline autoPlay muted loop />
          </aside>
        )}

        <div
          ref={gameAreaRef}

          className={gameAreaClassName}
          onMouseDown={handleMouseDown}
          onMouseMove={handleMouseMove}
          onMouseUp={handleMouseUp}
          onMouseLeave={handleMouseUp}
          onTouchStart={handleTouchStart}
          onTouchMove={handleTouchMove}
          onTouchEnd={handleTouchEnd}
          style={{ touchAction: "none" }} // Prevent default touch actions
        >
        <maintenance-widget />

        {!playerName && (
          <div className="ready-overlay">
            <h2 className="ready-overlay__title">Ready To Play?</h2>
            <p className="ready-overlay__subtitle">Enter your name below</p>
            <form className="ready-overlay__form" onSubmit={handleNameSubmit}>
              <input
                id="player-name"
                className="player-name-input"
                type="text"
                value={nameInput}
                onChange={handleNameChange}
                placeholder="ENTER NAME"
                maxLength={32}
                required
              />
              <button type="submit" className="pixel-button">START</button>
            </form>
            <p className="ready-overlay__hint">The game starts after you add your name</p>
          </div>
        )}
        {oranges.map((orange) => (
          <div key={orange.id}>
            <div
              className={`orange ${orange.sliced ? "sliced" : ""}`}
              style={
                {
                  width: orangeSize,
                  height: orangeSize,
                  "--rotation": `${orange.rotation}deg`,
                  transform: `translateX(${orange.x}px) translateY(${orange.y}px) translate(-50%, -50%) rotate(${orange.rotation}deg)`,
                } as React.CSSProperties
              }
            />
            {orange.sliced && (
              <>
                <div
                  className={`orange half top`}
                  style={
                    {
                      width: orangeSize,
                      height: orangeSize,
                      "--x-offset": `${orange.x}px`,
                      "--y-offset": `${orange.y}px`,
                      "--rotation": `${orange.rotation}deg`,
                      "--fly-distance": "-100px",
                      transform: `translate(-50%, -50%) rotate(${orange.rotation}deg)`,
                    } as React.CSSProperties
                  }
                />
                <div
                  className={`orange half bottom`}
                  style={
                    {
                      width: orangeSize,
                      height: orangeSize,
                      "--x-offset": `${orange.x}px`,
                      "--y-offset": `${orange.y}px`,
                      "--rotation": `${orange.rotation}deg`,
                      "--fly-distance": "100px",
                      transform: `translate(-50%, -50%) rotate(${orange.rotation}deg)`,
                    } as React.CSSProperties
                  }
                />
              </>
            )}
          </div>
        ))}
        {bombs.map((bomb) => (
          <div key={bomb.id}>
            <div
              className={`bomb ${bomb.sliced ? "sliced" : ""}`}
              style={
                {
                  width: bombSize,
                  height: bombSize,
                  "--rotation": `${bomb.rotation}deg`,
                  transform: `translateX(${bomb.x}px) translateY(${bomb.y}px) translate(-50%, -50%) rotate(${bomb.rotation}deg)`,
                } as React.CSSProperties
              }
            />
            {bomb.sliced && (
              <>
                <div
                  className="bomb-half top"
                  style={
                    {
                      width: bombSize,
                      height: bombSize,
                      "--x-offset": `${bomb.x}px`,
                      "--y-offset": `${bomb.y}px`,
                      "--rotation": `${bomb.rotation}deg`,
                      "--fly-distance": "-50px",
                      transform: `translateX(${bomb.x}px) translateY(${bomb.y}px) translate(-50%, -50%) rotate(${bomb.rotation}deg)`,
                    } as React.CSSProperties
                  }
                />
                <div
                  className="bomb-half bottom"
                  style={
                    {
                      width: bombSize,
                      height: bombSize,
                      "--x-offset": `${bomb.x}px`,
                      "--y-offset": `${bomb.y}px`,
                      "--rotation": `${bomb.rotation}deg`,
                      "--fly-distance": "50px",
                      transform: `translateX(${bomb.x}px) translateY(${bomb.y}px) translate(-50%, -50%) rotate(${bomb.rotation}deg)`,
                    } as React.CSSProperties
                  }
                />
              </>
            )}
          </div>
        ))}
        {scorePopups.map((popup) => (
          <div
            key={popup.id}
            className={`score-popup score-popup--${popup.variant}`}
            style={{ left: popup.x, top: popup.y }}
          >
            {popup.text}
          </div>
        ))}
        {juiceParticles.map((particle) => (
          <div
            key={particle.id}
            className="orange-juice"
            style={
              {
                /*left: `${particle.x}px`,
top: `${particle.y}px`,*/
                transform: `translateX(${particle.x}px) translateY(${particle.y}px)`,
                opacity: Math.max(0, 1 - particle.time / 1.5),
              } as React.CSSProperties
            }
          />
        ))}
        {explosionParticles.map((particle) => (
          <div
            key={particle.id}
            style={{
              position: "absolute",
              left: `${particle.x}px`,
              top: `${particle.y}px`,
              width: `${particle.size}px`,
              height: `${particle.size}px`,
              backgroundColor: particle.color,
              borderRadius: "50%",
              transform: "translate(-50%, -50%)",
              opacity: Math.max(0, 1 - particle.time / 1),
              boxShadow: `0 0 ${particle.size * 2}px ${particle.color}`,
              transition: "opacity 0.1s ease-out",
            }}
          />
        ))}

        </div>

        {!isMobileLayout && isSecretVideoOpen && secretVideoSources[1] && (
          <aside className="secret-video-panel secret-video-panel--right" aria-label="Secret Video">
            <video src={secretVideoSources[1]} playsInline autoPlay muted loop />
          </aside>
        )}
      </div>
      {(!isMobileLayout || !isScoreboardCollapsed) && (
        <footer className={`nes-hud nes-hud--footer${isMobileLayout ? " nes-hud--footer-mobile" : ""}`}>
          <div className="nes-hud__panel nes-hud__panel--pixel">
            <div className="nes-hud__grid">
            <div className="nes-hud__card nes-hud__card--player">
              <div className="nes-hud__title">PLAYER</div>
              <div className="nes-hud__score nes-hud__score--player">{playerName || "---"}</div>
              {playerName && (
                <button
                  type="button"
                  className="pixel-button pixel-button--ghost pixel-button--compact"
                  onClick={handleLogout}
                >
                  SWITCH
                </button>
              )}
              <div className="nes-hud__player-subtitle">
                <span>This is a purely experimental project, it's not connected to any airdrop or token rewards.</span>
              </div>
            </div>
            <div className="nes-hud__card nes-hud__card--score">
              <div className="nes-hud__title">SCORE</div>
              <div className={`nes-hud__score ${isScoreShaking ? "is-shaking" : ""}`}>{noteBalance.toLocaleString()}</div>
              <button
                type="button"
                className="pixel-button pixel-button--ghost pixel-button--compact"
                onClick={handleOpenManageModal}
                disabled={!playerName}
              >
                MANAGE
              </button>
              <div className="nes-hud__score-subtitle">
                <span>Your score lives only in your browser and can reset anytime.</span>
              </div>
            </div>
          </div>
          <div className="nes-hud__status">
            <div className={`nes-hud__status-item nes-hud__status-item--penalty ${bombPenalty > 0 ? "is-active" : ""}`}>
              <div className="status-box status-box--penalty">
                <div className="status-box__label">Penalty</div>
                <div className="status-box__row">
                  <div className="status-box__value">{penaltyDisplayText}</div>
                  <div className="status-box__meter">
                    <div
                      className="status-box__meter-fill"
                      style={{ width: `${penaltyMeterPercent * 100}%` }}
                    />
                  </div>
                </div>
              </div>
            </div>
            <div
              className={`nes-hud__status-item ${
                submissionError ? "nes-hud__status-item--error is-active" : "nes-hud__status-item--rate is-active"
              }`}
            >
              {submissionError ? (
                submissionError
              ) : (
                <div className="status-box status-box--rate">
                  <div className="status-box__label">Slice speed</div>
                  <div className="status-box__row">
                    <div className="status-box__value">{debouncedSliceSpeed.toFixed(2)} /s</div>
                    <div className="status-box__meter">
                      <div
                        className="status-box__meter-fill"
                        style={{ width: `${Math.min((debouncedSliceSpeed / MAX_SLICE_SPEED) * 100, 100)}%` }}
                      />
                    </div>
                  </div>
                </div>
              )}
            </div>
          </div>
          </div>
        </footer>
      )}

      {isMobileLayout && (
        <div className="mobile-scoreboard-toggle">
          <button
            type="button"
            className="pixel-button pixel-button--ghost mobile-scoreboard-toggle__button"
            onClick={handleToggleScoreboard}
          >
            {isScoreboardCollapsed ? "Show Scoreboard" : "Hide Scoreboard"}
          </button>
        </div>
      )}

      {isManageModalOpen && playerName && (
        <ManageNotesModal playerName={playerName} notes={storedNotes} onClose={handleCloseManageModal} />
      )}
      {debugMode && <DebugNotesPanel notes={storedNotes} onClear={clearNotes} />}

    </div>
  );
}

export default App;
