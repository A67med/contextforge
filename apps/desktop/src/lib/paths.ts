import { useQuery } from "@tanstack/react-query";
import { api } from "./api";

/** Show paths under the user's home folder as `~\…` (shorter, and keeps the account name off screen). */
export function useTilde(): (p: string | null | undefined) => string {
  const { data } = useQuery({ queryKey: ["diagnostics"], queryFn: api.diagnostics, staleTime: Infinity });
  const home = data?.homeDir?.replace(/[\\/]+$/, "");
  return (p) => {
    if (!p) return "";
    if (!home) return p;
    const norm = (s: string) => s.replace(/\//g, "\\").toLowerCase();
    if (norm(p).startsWith(norm(home) + "\\") || norm(p) === norm(home)) return "~" + p.slice(home.length);
    return p;
  };
}

/** Replace every occurrence of the home folder inside free text (e.g. a config diff preview). */
export function useTildeText(): (text: string) => string {
  const { data } = useQuery({ queryKey: ["diagnostics"], queryFn: api.diagnostics, staleTime: Infinity });
  const home = data?.homeDir?.replace(/[\\/]+$/, "");
  return (text) => {
    if (!home) return text;
    const variants = [home, home.replace(/\\/g, "\\\\"), home.replace(/\\/g, "/")];
    let out = text;
    for (const v of variants) out = out.split(v).join("~").split(v.toLowerCase()).join("~");
    return out;
  };
}
