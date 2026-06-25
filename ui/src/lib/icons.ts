import { Broom, Files, GitBranch, Stack, Recycle } from "phosphor-svelte";
import type { Component } from "svelte";
import type { ServiceId } from "./api";

export const serviceIcon: Record<ServiceId, Component> = {
  temp: Broom,
  app_cache: Recycle,
  big_files: Files,
  git_repos: GitBranch,
  dev_cache: Stack,
};
