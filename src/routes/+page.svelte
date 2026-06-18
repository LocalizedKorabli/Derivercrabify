<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  const win = getCurrentWindow();

  interface InstallationInfo {
    version_folder: string;
    text_installed: boolean;
    chat_installed: boolean;
  }

  interface InstanceInfo {
    path: string;
    version_folders: string[];
    installations: InstallationInfo[];
  }

  interface ProgressPayload {
    message: string;
    downloaded: number;
    total: number;
  }

  let instances = $state<InstanceInfo[]>([]);
  let loading = $state(false);
  let scanning = $state(true);
  let error = $state("");
  let statusMsg = $state("");
  let showAddModal = $state(false);
  let manualPath = $state("");
  let addError = $state("");
  let progressMsg = $state("");
  let progressDownloaded = $state(0);
  let progressTotal = $state(0);
  let progressPercent = $derived(
    progressTotal > 0 ? Math.round((progressDownloaded / progressTotal) * 100) : 0
  );
  let appVersion = $state("");
  let updateVersion = $state("");
  let updateDownloading = $state(false);
  let updatePercent = $state(0);
  let checkingUpdate = $state(false);
  let textAnticensor = $state(true);
  let chatAnticensor = $state(true);

  async function scan() {
    scanning = true;
    error = "";
    try {
      instances = await invoke("scan_instances");
      if (instances.length === 0) {
        error = "未找到战舰世界国服客户端实例。请确保已安装战舰世界国服。";
      }
    } catch (e: any) {
      error = "扫描失败：" + String(e);
      instances = [];
    } finally {
      scanning = false;
    }
  }

  async function handleAdd() {
    addError = "";
    const trimmed = manualPath.trim();
    if (!trimmed) {
      addError = "请输入安装路径";
      return;
    }

    try {
      const info: InstanceInfo = await invoke("add_instance", { path: trimmed });
      instances = [...instances, info];
      showAddModal = false;
      manualPath = "";
      statusMsg = "已添加实例：" + info.path;
    } catch (e: any) {
      addError = String(e);
    }
  }

  async function handleInstall(instancePath: string) {
    loading = true;
    error = "";
    statusMsg = "";
    progressMsg = "";
    progressDownloaded = 0;
    progressTotal = 0;

    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<ProgressPayload>("locale-progress", (event) => {
        progressMsg = event.payload.message;
        progressDownloaded = event.payload.downloaded;
        progressTotal = event.payload.total;
      });

      const results: InstallationInfo[] = await invoke("install_locale_pack", {
        instancePath,
        textAnticensor,
        chatAnticensor,
      });

      instances = instances.map((inst) => {
        if (inst.path === instancePath) {
          return { ...inst, installations: results };
        }
        return inst;
      });

      statusMsg = "安装完成！";
    } catch (e: any) {
      error = "安装失败：" + String(e);
    } finally {
      loading = false;
      if (unlisten) unlisten();
    }
  }

  async function handleRefresh(instancePath: string) {
    try {
      const info: InstanceInfo = await invoke("refresh_instance", {
        path: instancePath,
      });
      instances = instances.map((inst) =>
        inst.path === instancePath ? info : inst
      );
    } catch (e: any) {
      error = "刷新失败：" + String(e);
    }
  }

  onMount(() => {
    scan();
    getVersion();
    checkUpdateSilent();

    const cancel = listen<{ percent: number }>("update-progress", (event) => {
      updatePercent = event.payload.percent;
    });
    cancel.then((fn) => unlisteners.push(fn));
  });

  let unlisteners: (() => void)[] = [];

  async function getVersion() {
    try {
      appVersion = await invoke<string>("get_app_version");
    } catch {}
  }

  async function checkUpdateSilent() {
    try {
      const info = await invoke<{ version: string; path: string } | null>("check_update");
      if (info) updateVersion = info.version;
    } catch {}
  }

  async function checkUpdate() {
    checkingUpdate = true;
    try {
      const info = await invoke<{ version: string; path: string } | null>("check_update");
      if (info) {
        updateVersion = info.version;
      } else {
        updateVersion = "";
        statusMsg = "已是最新版";
        setTimeout(() => { statusMsg = ""; }, 3000);
      }
    } catch (e: any) {
      statusMsg = "检查更新失败: " + String(e);
      setTimeout(() => { statusMsg = ""; }, 5000);
    } finally {
      checkingUpdate = false;
    }
  }

  async function handleUpdate() {
    updateDownloading = true;
    updatePercent = 0;
    try {
      const info = await invoke<{ version: string; path: string }>("check_update");
      if (info) {
        await invoke("install_update", { downloadUrl: info.path });
      }
    } catch {
      updateDownloading = false;
    }
  }
</script>

<div data-tauri-drag-region class="h-9 flex items-center justify-between px-4">
  <span class="text-xs text-base-content/40 pointer-events-none">Derivercrabify</span>
  <div class="flex items-center gap-0 -mr-2">
    <button class="h-9 w-11 text-xs text-base-content/30 hover:text-base-content hover:bg-base-300 transition-colors" onclick={async () => await win.minimize()}>─</button>
    <button class="h-9 w-11 text-xs text-base-content/30 hover:text-base-content hover:bg-base-300 transition-colors" onclick={async () => await win.toggleMaximize()}>□</button>
    <button class="h-9 w-11 text-xs text-base-content/30 hover:text-white hover:bg-red-700 transition-colors" onclick={async () => await win.close()}>×</button>
  </div>
</div>

<main class="mx-auto max-w-xl px-6 py-6">
  {#if updateVersion}
    <div class="mb-5 flex items-center justify-between border border-base-300 px-4 py-3">
      <span class="text-sm">新版本 v{updateVersion} 可用</span>
      {#if updateDownloading}
        <div class="flex items-center gap-2">
          <progress class="w-20 h-0.5 [&::-webkit-progress-bar]:bg-base-300 [&::-webkit-progress-value]:bg-base-content" value={updatePercent} max="100"></progress>
          <span class="text-xs text-base-content/40">{updatePercent}%</span>
        </div>
      {:else}
        <button class="text-xs text-base-content/50 hover:text-base-content transition-colors" onclick={handleUpdate}>更新</button>
      {/if}
    </div>
  {/if}

  <div class="mb-6 flex items-center gap-4 border-b border-base-300 pb-3">
    <button class="text-xs text-base-content/50 hover:text-base-content transition-colors" onclick={scan} disabled={scanning}>
      {scanning ? "扫描中…" : "重新扫描"}
    </button>
    <span class="text-base-300">/</span>
    <button class="text-xs text-base-content/50 hover:text-base-content transition-colors" onclick={() => (showAddModal = true)}>
      手动添加
    </button>
    <span class="text-base-300">/</span>
    <button class="text-xs text-base-content/50 hover:text-base-content transition-colors" onclick={checkUpdate} disabled={checkingUpdate}>
      {checkingUpdate ? "检查中…" : "检查更新"}
    </button>
    <span class="flex-1"></span>
    <span class="text-xs text-base-content/30">{appVersion ? "v" + appVersion : ""}</span>
  </div>

  {#if scanning}
    <div class="py-16 text-center text-xs text-base-content/40">
      <span class="animate-pulse">扫描注册表中…</span>
    </div>
  {:else if error && instances.length === 0}
    <p class="text-xs text-base-content/40">{error}</p>
  {:else}
    {#if statusMsg}
      <p class="mb-4 text-xs text-base-content/40">{statusMsg}</p>
    {/if}

    {#if error}
      <p class="mb-4 text-xs text-error">{error}</p>
    {/if}

    {#if loading && progressMsg}
      <div class="mb-6 border border-base-300 px-4 py-3">
        <div class="flex items-center justify-between mb-2">
          <span class="text-xs text-base-content/50">{progressMsg}</span>
          {#if progressTotal > 0}
            <span class="text-xs text-base-content/40">{progressPercent}%</span>
          {:else}
            <span class="text-xs text-base-content/30 animate-pulse">处理中</span>
          {/if}
        </div>
        {#if progressTotal > 0}
          <progress class="w-full h-0.5 [&::-webkit-progress-bar]:bg-base-300 [&::-webkit-progress-value]:bg-base-content" value={progressDownloaded} max={progressTotal}></progress>
        {:else}
          <progress class="w-full h-0.5 [&::-webkit-progress-bar]:bg-base-300 [&::-webkit-progress-value]:bg-base-content" value={null}></progress>
        {/if}
      </div>
    {/if}

    <div class="space-y-4">
      {#each instances as inst (inst.path)}
        <div>
          <div class="flex items-start justify-between gap-3">
            <div class="min-w-0 flex-1">
              <p class="text-sm truncate text-base-content/50" title={inst.path}>{inst.path}</p>

              {#if inst.version_folders.length === 0}
                <p class="text-xs text-base-content/30 mt-1">未找到有效的游戏版本</p>
              {:else}
                <div class="mt-2 space-y-1">
                    {#each inst.installations as info}
                      <div class="flex items-center gap-3">
                        {#if info.text_installed || info.chat_installed}
                          <span class="w-0.5 h-3.5 bg-success shrink-0"></span>
                        {:else}
                          <span class="w-0.5 h-3.5 bg-base-300 shrink-0"></span>
                        {/if}
                        <span class="text-xs text-base-content/40">v{info.version_folder}</span>
                        {#if info.text_installed && info.chat_installed}
                          <span class="text-xs text-base-content/30">文本反和谐，聊天反和谐</span>
                        {:else if info.text_installed}
                          <span class="text-xs text-base-content/30">文本反和谐</span>
                        {:else if info.chat_installed}
                          <span class="text-xs text-base-content/30">聊天反和谐</span>
                        {:else}
                          <span class="text-xs text-base-content/30">未安装</span>
                        {/if}
                      </div>
                    {/each}
                </div>
              {/if}
            </div>

            <div class="flex items-center gap-2 shrink-0">
              <button class="text-xs text-base-content/30 hover:text-base-content/60 transition-colors" onclick={() => handleRefresh(inst.path)} title="刷新">
                <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                </svg>
              </button>
              {#if inst.version_folders.length > 0}
                <div class="flex flex-col items-end gap-1">
                  <label class="flex items-center gap-1.5 cursor-pointer">
                    <input type="checkbox" class="w-3 h-3 accent-current" bind:checked={textAnticensor} />
                    <span class="text-[11px] text-base-content/30">文本反和谐</span>
                  </label>
                  <label class="flex items-center gap-1.5 cursor-pointer">
                    <input type="checkbox" class="w-3 h-3 accent-current" bind:checked={chatAnticensor} />
                    <span class="text-[11px] text-base-content/30">聊天反和谐</span>
                  </label>
                  <button class="text-xs text-base-content/50 hover:text-base-content transition-colors" onclick={() => handleInstall(inst.path)} disabled={loading}>
                    {loading ? "安装中…" : "安装"}
                  </button>
                </div>
              {/if}
            </div>
          </div>
          <div class="border-b border-base-300 mt-3"></div>
        </div>
      {/each}
    </div>
  {/if}
</main>

{#if showAddModal}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div class="fixed inset-0 bg-base-300/60 z-50 flex items-end justify-center pb-16" onclick={() => { showAddModal = false; manualPath = ""; addError = ""; }}>
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="w-full max-w-md bg-base-100 border border-base-300 px-6 py-5" onclick={(e) => e.stopPropagation()}>
      <h3 class="text-sm mb-4">手动添加战舰世界国服实例</h3>
      <p class="text-xs text-base-content/50 mb-4">输入战舰世界国服客户端的安装目录</p>
      <input
        type="text"
        placeholder="例如 D:\Games\World of Warships 360"
        class="w-full bg-transparent border border-base-300 px-3 py-2 text-sm text-base-content placeholder:text-base-content/30 focus:border-base-content/50 transition-colors"
        bind:value={manualPath}
        onkeydown={(e: KeyboardEvent) => { if (e.key === "Enter") handleAdd(); }}
      />
      {#if addError}
        <p class="text-xs text-error mt-2">{addError}</p>
      {/if}
      <div class="flex justify-end gap-3 mt-5">
        <button class="text-xs text-base-content/40 hover:text-base-content/60 transition-colors" onclick={() => { showAddModal = false; manualPath = ""; addError = ""; }}>取消</button>
        <button class="text-xs text-base-content/70 hover:text-base-content transition-colors" onclick={handleAdd}>添加</button>
      </div>
    </div>
  </div>
{/if}
