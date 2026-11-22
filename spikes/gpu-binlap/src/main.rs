//! Spike: the stacker's first pass per frame (flat calibration, 2x2 bin, Laplacian sums) on the GPU against the NEON kernels.
use std::time::Instant;

const W: usize = 1944;
const H: usize = 1472;
const SHADER: &str = r#"
struct Params { w: u32, h: u32, ow: u32, oh: u32 };
@group(0) @binding(0) var<storage, read> raw: array<u32>;
@group(0) @binding(1) var<storage, read> inv_flat: array<f32>;
@group(0) @binding(2) var<storage, read_write> binned: array<f32>;
@group(0) @binding(3) var<uniform> p: Params;

@compute @workgroup_size(16, 16)
fn bin2(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= p.ow || id.y >= p.oh) { return; }
    let i0 = (id.y * 2u) * p.w + id.x * 2u;
    let i1 = i0 + p.w;
    let a = raw[i0 >> 1u];
    let b = raw[i1 >> 1u];
    let v = f32(a & 0xffffu) * inv_flat[i0] + f32(a >> 16u) * inv_flat[i0 + 1u] + f32(b & 0xffffu) * inv_flat[i1] + f32(b >> 16u) * inv_flat[i1 + 1u];
    binned[id.y * p.ow + id.x] = 0.25 * v;
}
"#;
const SHADER2: &str = r#"
struct Params { w: u32, h: u32, ow: u32, oh: u32 };
@group(0) @binding(0) var<storage, read> img: array<f32>;
@group(0) @binding(1) var<storage, read_write> partial: array<f32>;
@group(0) @binding(2) var<uniform> p: Params;
var<workgroup> sh: array<f32, 768>;

@compute @workgroup_size(16, 16)
fn lap(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) li: u32, @builtin(workgroup_id) wg: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    var s = 0.0; var s2 = 0.0; var sm = 0.0;
    if (id.x >= 1u && id.y >= 1u && id.x < p.ow - 1u && id.y < p.oh - 1u) {
        let i = id.y * p.ow + id.x;
        let c = img[i];
        let l = 4.0 * c - img[i - 1u] - img[i + 1u] - img[i - p.ow] - img[i + p.ow];
        s = l; s2 = l * l; sm = c;
    }
    sh[li] = s; sh[256u + li] = s2; sh[512u + li] = sm;
    workgroupBarrier();
    for (var st = 128u; st > 0u; st = st >> 1u) {
        if (li < st) { sh[li] += sh[li + st]; sh[256u + li] += sh[256u + li + st]; sh[512u + li] += sh[512u + li + st]; }
        workgroupBarrier();
    }
    if (li == 0u) {
        let o = (wg.y * nwg.x + wg.x) * 3u;
        partial[o] = sh[0]; partial[o + 1u] = sh[256]; partial[o + 2u] = sh[512];
    }
}
"#;

fn frame(seed: u32) -> Vec<u16> {
    let mut s = seed.wrapping_mul(2654435761).wrapping_add(1);
    (0..W * H).map(|i| { s = s.wrapping_mul(1664525).wrapping_add(1013904223); (2000 + ((i % W) as u32 / 3) + (s >> 22)) as u16 }).collect()
}

fn main() {
    let raw = frame(1);
    let inv_flat: Vec<f32> = (0..W * H).map(|i| 1.0 / (0.7 + 0.3 * ((i / W) as f32 / H as f32))).collect();
    let (ow, oh) = (W / 2, H / 2);
    let mask = vec![1.0f32; ow * oh];
    // the CPU reference and the NEON kernels from the stacker, on this machine
    let mut cpu = vec![0f32; ow * oh];
    let t = Instant::now();
    let n = 20;
    for _ in 0..n {
        rustflare_stack::simd::bin2_calibrated(&raw, None, Some(&inv_flat), W, H, &mut cpu);
        std::hint::black_box(rustflare_stack::simd::lap_stats(&cpu, &mask, ow, oh));
    }
    let cpu_ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    let cpu_stats = rustflare_stack::simd::lap_stats(&cpu, &mask, ow, oh);

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).expect("no GPU adapter");
    let info = adapter.get_info();
    println!("GPU: {} ({:?}, driver {} {})", info.name, info.backend, info.driver, info.driver_info);
    let lim = adapter.limits();
    println!("  max workgroup invocations {}, shared memory {} B, max storage buffer {} MB", lim.max_compute_invocations_per_workgroup, lim.max_compute_workgroup_storage_size, lim.max_storage_buffer_binding_size / 1_000_000);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { required_limits: wgpu::Limits { max_storage_buffer_binding_size: 128 << 20, ..wgpu::Limits::downlevel_defaults() }, ..Default::default() }, None)).expect("device");

    let buf = |label: &str, size: usize, usage: wgpu::BufferUsages| device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: size as u64, usage, mapped_at_creation: false });
    use wgpu::BufferUsages as U;
    let raw_b = buf("raw", W * H * 2, U::STORAGE | U::COPY_DST);
    let flat_b = buf("flat", W * H * 4, U::STORAGE | U::COPY_DST);
    let binned_b = buf("binned", ow * oh * 4, U::STORAGE | U::COPY_SRC);
    let (gx, gy) = ((ow as u32 + 15) / 16, (oh as u32 + 15) / 16);
    let partial_b = buf("partial", (gx * gy * 3) as usize * 4, U::STORAGE | U::COPY_SRC);
    let read_partial = buf("read_partial", (gx * gy * 3) as usize * 4, U::MAP_READ | U::COPY_DST);
    let read_binned = buf("read_binned", ow * oh * 4, U::MAP_READ | U::COPY_DST);
    let params = device.create_buffer(&wgpu::BufferDescriptor { label: Some("p"), size: 16, usage: U::UNIFORM | U::COPY_DST, mapped_at_creation: false });
    queue.write_buffer(&params, 0, bytemuck::cast_slice(&[W as u32, H as u32, ow as u32, oh as u32]));
    queue.write_buffer(&flat_b, 0, bytemuck::cast_slice(&inv_flat));

    let pipe = |src: &str, entry: &str| device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(entry), layout: None, module: &device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(entry), source: wgpu::ShaderSource::Wgsl(src.into()) }), entry_point: Some(entry), compilation_options: Default::default(), cache: None });
    let (p1, p2) = (pipe(SHADER, "bin2"), pipe(SHADER2, "lap"));
    let bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p1.get_bind_group_layout(0), entries: &[
        wgpu::BindGroupEntry { binding: 0, resource: raw_b.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: flat_b.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 2, resource: binned_b.as_entire_binding() }, wgpu::BindGroupEntry { binding: 3, resource: params.as_entire_binding() }] });
    let bg2 = device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout: &p2.get_bind_group_layout(0), entries: &[
        wgpu::BindGroupEntry { binding: 0, resource: binned_b.as_entire_binding() }, wgpu::BindGroupEntry { binding: 1, resource: partial_b.as_entire_binding() }, wgpu::BindGroupEntry { binding: 2, resource: params.as_entire_binding() }] });
    let raw_bytes: &[u8] = bytemuck::cast_slice(&raw);

    let run = |upload: bool, read_img: bool| -> (f64, Option<Vec<f32>>) {
        if upload {
            queue.write_buffer(&raw_b, 0, raw_bytes);
        }
        let mut enc = device.create_command_encoder(&Default::default());
        { let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p1); c.set_bind_group(0, &bg1, &[]); c.dispatch_workgroups(gx, gy, 1); }
        { let mut c = enc.begin_compute_pass(&Default::default()); c.set_pipeline(&p2); c.set_bind_group(0, &bg2, &[]); c.dispatch_workgroups(gx, gy, 1); }
        enc.copy_buffer_to_buffer(&partial_b, 0, &read_partial, 0, (gx * gy * 3) as u64 * 4);
        if read_img { enc.copy_buffer_to_buffer(&binned_b, 0, &read_binned, 0, (ow * oh * 4) as u64); }
        queue.submit(Some(enc.finish()));
        let (s1, s2) = (read_partial.slice(..), read_binned.slice(..));
        s1.map_async(wgpu::MapMode::Read, |_| {});
        if read_img { s2.map_async(wgpu::MapMode::Read, |_| {}); }
        device.poll(wgpu::Maintain::Wait);
        let d = s1.get_mapped_range();
        let parts: &[f32] = bytemuck::cast_slice(&d);
        let (mut a, mut b, mut c) = (0.0f64, 0.0f64, 0.0f64);
        for k in 0..(gx * gy) as usize { a += parts[3 * k] as f64; b += parts[3 * k + 1] as f64; c += parts[3 * k + 2] as f64; }
        drop(d); read_partial.unmap();
        let img = if read_img { let d = s2.get_mapped_range(); let v: Vec<f32> = bytemuck::cast_slice(&d).to_vec(); drop(d); read_binned.unmap(); Some(v) } else { None };
        let _ = (a, b);
        (c, img)
    };
    // correctness first
    let (_, img) = run(true, true);
    let img = img.unwrap();
    let worst = img.iter().zip(&cpu).map(|(a, b)| (a - b).abs() / b.abs().max(1.0)).fold(0.0f32, f32::max);
    let (a, b, c) = { let _ = run(true, false); let d = read_partial.slice(..); let _ = d; (0.0, 0.0, 0.0) };
    let _ = (a, b, c);
    println!("check: binned image max relative difference GPU vs CPU {worst:.2e}");
    let _ = cpu_stats;

    let n = 20;
    let time = |label: &str, f: &dyn Fn()| -> f64 { f(); let t = Instant::now(); for _ in 0..n { f(); } let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64; println!("  {label:62} {ms:7.2} ms / frame"); ms };
    println!("\nper frame ({W}x{H} u16): flat calibration + 2x2 bin + Laplacian sums");
    println!("  {:62} {cpu_ms:7.2} ms / frame", "CPU: NEON kernels of the stacker (one core)");
    let g0 = time("GPU: compute only, results stay on the GPU (no upload/readback)", &|| { let _ = run(false, false); });
    let g1 = time("GPU: upload the frame + compute + read the sums back", &|| { let _ = run(true, false); });
    let g2 = time("GPU: upload + compute + read the binned image back as well", &|| { let _ = run(true, true); });
    println!("\nspeed-up over the CPU kernel (one core): compute only {:.1}x, with upload and sums {:.1}x, with the binned image back {:.1}x", cpu_ms / g0, cpu_ms / g1, cpu_ms / g2);
}
