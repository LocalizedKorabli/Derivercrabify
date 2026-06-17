<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  interface InstallationInfo {
    version_folder: string;
    installed: boolean;
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

  async function scan() {
    scanning = true;
    error = "";
    try {
      instances = await invoke("scan_instances");
      if (instances.length === 0) {
        error = "未找到CN360战舰世界客户端实例。请确保已安装360战舰世界。";
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

<main class="container mx-auto max-w-2xl p-6">
  <h1 class="text-3xl font-bold text-center mb-2">Derivercrabify</h1>
  <p class="text-center text-base-content/60 mb-8">
    CN360战舰世界反和谐工具
  </p>

  {#if updateVersion}
    <div role="alert" class="alert alert-info mb-4">
      <span>发现新版本 v{updateVersion}</span>
      <div class="flex gap-2">
        {#if updateDownloading}
          <div class="flex items-center gap-2">
            <progress class="progress progress-info w-24" value={updatePercent} max="100"></progress>
            <span class="text-sm">{updatePercent}%</span>
          </div>
        {:else}
          <button class="btn btn-sm btn-primary" onclick={handleUpdate}>更新</button>
        {/if}
      </div>
    </div>
  {/if}

  <div class="flex justify-center gap-3 mb-6">
    <button class="btn btn-outline btn-sm" onclick={scan} disabled={scanning}>
      {scanning ? "扫描中..." : "重新扫描"}
    </button>
    <button class="btn btn-primary btn-sm" onclick={() => (showAddModal = true)}>
      手动添加实例
    </button>
    <button class="btn btn-ghost btn-sm" onclick={checkUpdate} disabled={checkingUpdate}>
      {checkingUpdate ? "检查中..." : updateVersion ? "有新版本" : "检查更新"}
    </button>
  </div>

  {#if scanning}
    <div class="flex justify-center items-center gap-3 py-12">
      <span class="loading loading-spinner loading-lg"></span>
      <span class="text-base-content/60">正在扫描注册表...</span>
    </div>
  {:else if error && instances.length === 0}
    <div role="alert" class="alert alert-warning">
      <svg
        xmlns="http://www.w3.org/2000/svg"
        class="h-6 w-6 shrink-0 stroke-current"
        fill="none"
        viewBox="0 0 24 24"
      >
        <path
          stroke-linecap="round"
          stroke-linejoin="round"
          stroke-width="2"
          d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z"
        />
      </svg>
      <span>{error}</span>
    </div>
  {:else}
    {#if statusMsg}
      <div role="alert" class="alert alert-success mb-4">
        <svg
          xmlns="http://www.w3.org/2000/svg"
          class="h-6 w-6 shrink-0 stroke-current"
          fill="none"
          viewBox="0 0 24 24"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
        <span>{statusMsg}</span>
      </div>
    {/if}

    {#if error}
      <div role="alert" class="alert alert-error mb-4">
        <svg
          xmlns="http://www.w3.org/2000/svg"
          class="h-6 w-6 shrink-0 stroke-current"
          fill="none"
          viewBox="0 0 24 24"
        >
          <path
            stroke-linecap="round"
            stroke-linejoin="round"
            stroke-width="2"
            d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z"
          />
        </svg>
        <span>{error}</span>
      </div>
    {/if}

    {#if loading && progressMsg}
      <div class="card bg-base-200 shadow-sm mb-4">
        <div class="card-body p-4">
          <div class="flex items-center justify-between mb-2">
            <span class="text-sm">{progressMsg}</span>
            {#if progressTotal > 0}
              <span class="text-sm text-base-content/60">{progressPercent}%</span>
            {:else}
              <span class="loading loading-spinner loading-xs"></span>
            {/if}
          </div>
          {#if progressTotal > 0}
            <progress
              class="progress progress-primary w-full"
              value={progressDownloaded}
              max={progressTotal}
            ></progress>
          {:else}
            <progress class="progress progress-primary w-full" value={null}></progress>
          {/if}
        </div>
      </div>
    {/if}

    <div class="space-y-4">
      {#each instances as inst (inst.path)}
        <div class="card bg-base-200 shadow-sm">
          <div class="card-body p-5">
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0 flex-1">
                <h3 class="card-title text-base truncate" title={inst.path}>
                  {inst.path}
                </h3>

                {#if inst.version_folders.length === 0}
                  <p class="text-sm text-base-content/50 mt-1">
                    未找到有效的游戏版本文件夹
                  </p>
                {:else}
                  <div class="mt-2 space-y-1.5">
                    {#each inst.installations as info}
                      <div class="flex items-center gap-2 text-sm">
                        <span
                          class="badge {info.installed
                            ? 'badge-success'
                            : 'badge-ghost'}"
                        >
                          v{info.version_folder}
                        </span>
                        <span class="text-base-content/60">
                          {info.installed ? "已安装" : "未安装"}
                        </span>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>

              <div class="flex flex-col gap-2 shrink-0">
                <button
                  class="btn btn-ghost btn-xs"
                  onclick={() => handleRefresh(inst.path)}
                  title="刷新"
                >
                  <svg
                    xmlns="http://www.w3.org/2000/svg"
                    class="h-4 w-4"
                    fill="none"
                    viewBox="0 0 24 24"
                    stroke="currentColor"
                  >
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      stroke-width="2"
                      d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15"
                    />
                  </svg>
                </button>
              </div>
            </div>

            {#if inst.version_folders.length > 0}
              <div class="card-actions mt-3">
                <button
                  class="btn btn-primary btn-sm"
                  onclick={() => handleInstall(inst.path)}
                  disabled={loading}
                >
                  {loading ? "安装中..." : "安装语言包"}
                </button>
              </div>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}

  <p class="text-center text-xs text-base-content/30 mt-8">
    v{appVersion}
  </p>
</main>

{#if showAddModal}
  <div class="modal modal-open">
    <div class="modal-box">
      <h3 class="text-lg font-bold mb-4">手动添加CN360实例</h3>
      <p class="text-sm text-base-content/60 mb-4">
        请输入战舰世界CN360客户端的安装目录（包含 wgc360_api.exe 和 bin 目录）。
      </p>
      <input
        type="text"
        placeholder="例如：D:\Games\World of Warships 360"
        class="input input-bordered w-full"
        bind:value={manualPath}
        onkeydown={(e: KeyboardEvent) => {
          if (e.key === "Enter") handleAdd();
        }}
      />
      {#if addError}
        <p class="text-error text-sm mt-2">{addError}</p>
      {/if}
      <div class="modal-action">
        <button class="btn btn-ghost" onclick={() => {
          showAddModal = false;
          manualPath = "";
          addError = "";
        }}>
          取消
        </button>
        <button class="btn btn-primary" onclick={handleAdd}>
          添加
        </button>
      </div>
    </div>
  </div>
{/if}
