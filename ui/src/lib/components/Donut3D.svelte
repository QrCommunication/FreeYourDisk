<script lang="ts">
  import { onMount, onDestroy, type Snippet } from "svelte";
  import * as THREE from "three";

  interface Segment {
    value: number;
    color: string;
  }

  let {
    segments = [],
    size = 320,
    children,
  }: { segments?: Segment[]; size?: number; children?: Snippet } = $props();

  let host: HTMLDivElement;
  let renderer: THREE.WebGLRenderer | undefined;
  let scene: THREE.Scene;
  let camera: THREE.PerspectiveCamera;
  let group: THREE.Group;
  let raf = 0;
  let ready = $state(false);

  const RADIUS = 1.0;
  const TUBE = 0.4;
  const TWO_PI = Math.PI * 2;

  function clearGroup(): void {
    while (group.children.length) {
      const child = group.children.pop() as THREE.Mesh;
      child.geometry?.dispose?.();
      (child.material as THREE.Material)?.dispose?.();
    }
  }

  function addArc(
    start: number,
    sweep: number,
    color: string,
    opacity = 1,
  ): void {
    if (sweep <= 0.0001) return;
    const tubular = Math.max(6, Math.round((sweep / TWO_PI) * 200));
    const geometry = new THREE.TorusGeometry(RADIUS, TUBE, 26, tubular, sweep);
    const material = new THREE.MeshStandardMaterial({
      color: new THREE.Color(color),
      roughness: 0.4,
      metalness: 0.15,
      transparent: opacity < 1,
      opacity,
    });
    const mesh = new THREE.Mesh(geometry, material);
    mesh.rotation.z = start;
    group.add(mesh);
  }

  function build(segs: Segment[]): void {
    if (!group) return;
    clearGroup();
    const positive = segs.filter((s) => s.value > 0);
    const total = positive.reduce((sum, s) => sum + s.value, 0);
    if (total <= 0) {
      addArc(0, TWO_PI, "#9aa4b2", 0.18);
      return;
    }
    const gap = positive.length > 1 ? 0.018 * TWO_PI : 0;
    let cursor = -Math.PI / 2; // start at the top
    for (const seg of positive) {
      const sweep = (seg.value / total) * TWO_PI;
      addArc(cursor + gap / 2, sweep - gap, seg.color);
      cursor += sweep;
    }
  }

  onMount(() => {
    scene = new THREE.Scene();
    camera = new THREE.PerspectiveCamera(38, 1, 0.1, 100);
    camera.position.set(0, 0, 5);

    renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.setSize(size, size);
    host.appendChild(renderer.domElement);

    group = new THREE.Group();
    group.rotation.x = -0.62; // tilt forward for 3D perspective
    scene.add(group);

    scene.add(new THREE.AmbientLight(0xffffff, 0.72));
    const key = new THREE.DirectionalLight(0xffffff, 1.15);
    key.position.set(2.5, 3.5, 4);
    scene.add(key);
    const rim = new THREE.DirectionalLight(0xffffff, 0.35);
    rim.position.set(-3, -1.5, 2);
    scene.add(rim);

    build(segments);
    ready = true;

    const reduce = window.matchMedia(
      "(prefers-reduced-motion: reduce)",
    ).matches;
    let t = 0;
    const loop = () => {
      raf = requestAnimationFrame(loop);
      if (!reduce) {
        t += 0.012;
        group.rotation.y = Math.sin(t) * 0.3;
      }
      renderer?.render(scene, camera);
    };
    loop();
  });

  $effect(() => {
    const next = segments;
    if (ready) build(next);
  });

  onDestroy(() => {
    cancelAnimationFrame(raf);
    renderer?.dispose();
    renderer?.forceContextLoss?.();
  });
</script>

<div
  class="relative grid place-items-center"
  style="width:{size}px;height:{size}px"
>
  <div bind:this={host} class="absolute inset-0"></div>
  <div class="pointer-events-none absolute inset-0 grid place-items-center">
    <div class="pointer-events-auto text-center">
      {@render children?.()}
    </div>
  </div>
</div>
