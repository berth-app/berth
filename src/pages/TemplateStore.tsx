import { useEffect, useState, useRef, useCallback } from "react";
import {
  Search,
  RefreshCw,
  Download,
  Star,
  Globe,
  Server,
  Bot,
  Brain,
  Loader2,
} from "lucide-react";
import {
  listStoreTemplates,
  searchStoreTemplates,
  installStoreTemplate,
  type TemplateItem,
  type TemplateCategory,
} from "../lib/invoke";
import { useToast } from "../components/Toast";

interface Props {
  onInstalled: (projectId: string) => void;
}

const RUNTIME_ICONS: Record<string, string> = {
  python: "Py",
  node: "JS",
  go: "Go",
  rust: "Rs",
  shell: "Sh",
  unknown: "?",
};

const RUNTIME_COLORS: Record<string, string> = {
  python: "bg-blue-500/10 text-blue-400 border-blue-500/20",
  node: "bg-green-500/10 text-green-400 border-green-500/20",
  go: "bg-cyan-500/10 text-cyan-400 border-cyan-500/20",
  rust: "bg-orange-500/10 text-orange-400 border-orange-500/20",
  shell: "bg-yellow-500/10 text-yellow-400 border-yellow-500/20",
  unknown: "bg-berth-surface-2 text-berth-text-tertiary border-berth-border",
};

const CATEGORY_ICONS: Record<string, typeof Globe> = {
  globe: Globe,
  server: Server,
  bot: Bot,
  brain: Brain,
};

export default function TemplateStore({ onInstalled }: Props) {
  const [templates, setTemplates] = useState<TemplateItem[]>([]);
  const [categories, setCategories] = useState<TemplateCategory[]>([]);
  const [selectedCategory, setSelectedCategory] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [installing, setInstalling] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const searchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const { toast } = useToast();

  const fetchTemplates = useCallback(
    async (category?: string | null, forceRefresh = false) => {
      try {
        const result = await listStoreTemplates(
          category ?? undefined,
          forceRefresh
        );
        setCategories(result.categories);
        setTemplates(result.templates);
      } catch (e) {
        toast(
          `Failed to load templates: ${e}`,
          "error"
        );
      }
    },
    [toast]
  );

  useEffect(() => {
    setLoading(true);
    fetchTemplates(null, false).finally(() => setLoading(false));
  }, [fetchTemplates]);

  const handleCategoryChange = (cat: string | null) => {
    setSelectedCategory(cat);
    setSearchQuery("");
    setLoading(true);
    fetchTemplates(cat).finally(() => setLoading(false));
  };

  const handleSearch = (query: string) => {
    setSearchQuery(query);
    if (searchTimer.current) clearTimeout(searchTimer.current);
    if (!query.trim()) {
      fetchTemplates(selectedCategory);
      return;
    }
    searchTimer.current = setTimeout(async () => {
      setLoading(true);
      try {
        const result = await searchStoreTemplates(query);
        setTemplates(result.templates);
      } catch (e) {
        toast(`Search failed: ${e}`, "error");
      } finally {
        setLoading(false);
      }
    }, 300);
  };

  const handleRefresh = async () => {
    setRefreshing(true);
    await fetchTemplates(selectedCategory, true);
    setRefreshing(false);
  };

  const handleInstall = async (template: TemplateItem) => {
    setInstalling(template.id);
    try {
      const project = await installStoreTemplate(template.id);
      toast(`Installed "${template.name}"`, "success");
      onInstalled(project.id);
    } catch (e) {
      toast(`Install failed: ${e}`, "error");
    } finally {
      setInstalling(null);
    }
  };

  return (
    <div className="h-full flex flex-col">
      {/* Header */}
      <div
        data-tauri-drag-region
        className="flex items-center justify-between px-5 py-3 border-b border-berth-border shrink-0"
      >
        <h1 className="text-base font-semibold text-berth-text">
          Template Store
        </h1>
        <button
          onClick={handleRefresh}
          disabled={refreshing}
          className="btn btn-ghost btn-icon"
          title="Refresh catalog"
        >
          <RefreshCw
            size={14}
            strokeWidth={1.75}
            className={refreshing ? "animate-spin" : ""}
          />
        </button>
      </div>

      {/* Search + Category filters */}
      <div className="px-5 pt-3 pb-2 space-y-2 shrink-0">
        <div className="relative">
          <Search
            size={14}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-berth-text-tertiary"
          />
          <input
            type="text"
            placeholder="Search templates..."
            value={searchQuery}
            onChange={(e) => handleSearch(e.target.value)}
            className="input pl-8 w-full"
          />
        </div>

        <div className="flex gap-1.5 flex-wrap">
          <button
            className={`px-2.5 py-1 rounded-full text-[11px] font-medium transition-colors ${
              selectedCategory === null
                ? "bg-berth-accent text-white"
                : "bg-berth-surface-2 text-berth-text-secondary hover:bg-berth-border"
            }`}
            onClick={() => handleCategoryChange(null)}
          >
            All
          </button>
          {categories.map((cat) => {
            const Icon = CATEGORY_ICONS[cat.icon] ?? Globe;
            return (
              <button
                key={cat.id}
                className={`px-2.5 py-1 rounded-full text-[11px] font-medium transition-colors inline-flex items-center gap-1 ${
                  selectedCategory === cat.id
                    ? "bg-berth-accent text-white"
                    : "bg-berth-surface-2 text-berth-text-secondary hover:bg-berth-border"
                }`}
                onClick={() => handleCategoryChange(cat.id)}
              >
                <Icon size={10} strokeWidth={2} />
                {cat.name}
              </button>
            );
          })}
        </div>
      </div>

      {/* Template grid */}
      <div className="flex-1 overflow-y-auto px-5 pb-5">
        {loading ? (
          <div className="flex items-center justify-center h-32 text-berth-text-tertiary text-sm">
            <Loader2 size={16} className="animate-spin mr-2" />
            Loading templates...
          </div>
        ) : templates.length === 0 ? (
          <div className="flex items-center justify-center h-32 text-berth-text-tertiary text-sm">
            No templates found
          </div>
        ) : (
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-3 pt-2">
            {templates.map((t) => (
              <TemplateCard
                key={t.id}
                template={t}
                installing={installing === t.id}
                onInstall={() => handleInstall(t)}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function TemplateCard({
  template,
  installing,
  onInstall,
}: {
  template: TemplateItem;
  installing: boolean;
  onInstall: () => void;
}) {
  return (
    <div
      className="rounded-lg border p-4 flex flex-col gap-2 transition-colors border-berth-border bg-berth-surface hover:border-berth-accent/30"
    >
      {/* Header row */}
      <div className="flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          <span
            className={`shrink-0 px-1.5 py-0.5 rounded text-[10px] font-mono font-bold border ${
              RUNTIME_COLORS[template.runtime] ?? RUNTIME_COLORS.unknown
            }`}
          >
            {RUNTIME_ICONS[template.runtime] ?? "?"}
          </span>
          <h3 className="text-[13px] font-semibold text-berth-text truncate">
            {template.name}
          </h3>
          {template.featured && (
            <Star
              size={12}
              strokeWidth={2}
              className="shrink-0 text-yellow-400 fill-yellow-400"
            />
          )}
        </div>
      </div>

      {/* Description */}
      <p className="text-[11px] text-berth-text-secondary leading-relaxed line-clamp-2">
        {template.description}
      </p>

      {/* Env var hints */}
      {template.env_vars.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {template.env_vars.map((ev) => (
            <span
              key={ev.key}
              className="px-1.5 py-0.5 rounded text-[9px] font-mono bg-berth-surface-2 text-berth-text-tertiary"
              title={ev.description}
            >
              {ev.key}
            </span>
          ))}
        </div>
      )}

      {/* Footer */}
      <div className="flex items-center justify-between mt-auto pt-1">
        <div className="flex items-center gap-2 text-[10px] text-berth-text-tertiary">
          {template.install_count > 0 && (
            <span className="inline-flex items-center gap-0.5">
              <Download size={10} strokeWidth={2} />
              {template.install_count}
            </span>
          )}
          <span>v{template.version}</span>
        </div>

        <button
          onClick={onInstall}
          disabled={installing}
          className="px-3 py-1 rounded-md text-[11px] font-medium transition-colors bg-berth-accent text-white hover:bg-berth-accent/90"
        >
          {installing ? (
            <Loader2 size={12} className="animate-spin" />
          ) : (
            "Install"
          )}
        </button>
      </div>
    </div>
  );
}
