import type { Title } from "../types";
export interface Experience {
  trailer_url: string | null;
  trailer_key: string | null;
  chapters: {
    start_time: string;
    end_time: string;
    tags?: { title?: string };
  }[];
  next_episode: Title | null;
  people: { name: string; role: string }[];
}
