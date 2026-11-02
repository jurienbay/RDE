// SPDX-License-Identifier: BSD-3-Clause
// Copyright (C) <2026>, <Edward O'Callaghan>, All rights reserved.

use ash::{Device, Entry, Instance, khr, vk};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use smithay::backend::allocator::dmabuf::AsDmabuf;
use smithay::backend::allocator::gbm::{GbmAllocator, GbmBuffer};
use smithay::backend::drm::DrmDeviceFd;
use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit::WinitGraphicsBackend;

use std::os::fd::BorrowedFd;
use std::os::linux::fs::MetadataExt;
use std::sync::Arc;

use crate::ActiveMenu;
use crate::WindowContext;
use crate::constants::*;
use crate::glsl::*;

pub struct WindowTexture {
    // Keep a shared reference to the device and descriptor pool for destruction
    device: Arc<Device>,
    descriptor_pool: vk::DescriptorPool,

    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub descriptor_set: vk::DescriptorSet,
    pub width: u32,
    pub height: u32,
    pub is_dmabuf: bool,
}

impl WindowTexture {
    /// Checks if an existing texture matches the requested specifications.
    pub fn matches_specs(&self, width: u32, height: u32, is_dmabuf: bool) -> bool {
        self.width == width && self.height == height && self.is_dmabuf == is_dmabuf
    }
}

impl Drop for WindowTexture {
    fn drop(&mut self) {
        unsafe {
            // Free the discrete descriptor set from the pool
            let _ = self.device.free_descriptor_sets(
                self.descriptor_pool,
                &[self.descriptor_set]
            );

            // Destroy the physical texture primitives
            self.device.destroy_image_view(self.view, None);
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);

            println!("DEBUG: WindowTexture for {}x{} successfully dropped via RAII.", self.width, self.height);
        }
    }
}

pub struct GbmRenderFrame {
    pub gbm_buffer: GbmBuffer,
    pub image: vk::Image,
    pub memory: vk::DeviceMemory,
    pub view: vk::ImageView,
    pub framebuffer: vk::Framebuffer,
}

pub struct VulkanEngine {
    _entry: Entry,
    pub instance: Instance,
    pub device: Arc<Device>,
    pub physical_device: vk::PhysicalDevice,
    pub queue_family_index: u32,
    pub graphics_queue: vk::Queue,
    pub render_node_or_gpu_device_handle: u64,

    // Window surface and swapchain
    pub surface_loader: khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    pub swapchain_loader: khr::swapchain::Device,
    pub swapchain: vk::SwapchainKHR,
    pub swapchain_images: Vec<vk::Image>,
    pub swapchain_image_views: Vec<vk::ImageView>,
    pub swapchain_extent: vk::Extent2D,

    // Headless Hardware Allocations tracking states
    pub render_frames: Vec<GbmRenderFrame>,
    pub current_frame_index: usize,

    // Pipeline infrastructure
    pub render_pass: vk::RenderPass,
    pub descriptor_set_layout: vk::DescriptorSetLayout, // Binds slot 0 layout specs
    pub pipeline_layout: vk::PipelineLayout,
    pub graphics_pipeline: vk::Pipeline,
    pub framebuffers: Vec<vk::Framebuffer>,

    // Command buffers & Synchronisation
    pub command_pool: vk::CommandPool,
    pub command_buffers: Vec<vk::CommandBuffer>,
    pub image_available_semaphore: vk::Semaphore,
    pub render_finished_semaphore: vk::Semaphore,
    pub in_flight_fence: vk::Fence,

    // Active Client Texture Allocations Management State
    pub texture_sampler: vk::Sampler,
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_set: vk::DescriptorSet,
    pub window_textures: std::sync::Mutex<std::collections::HashMap<u32, WindowTexture>>,
}

fn workspace_clear_colour(active_workspace: u32) -> [vk::ClearValue; 1] {
    // Authentic 1996 CDE Room Palette Scheme Matrix
    let background_color = match active_workspace {
        0 => [0.00, 0.47, 0.47, 1.0], // Room One: Canonical Desktop Teal
        1 => [0.40, 0.20, 0.30, 1.0], // Room Two: Classic Motif Maroon
        2 => [0.25, 0.35, 0.45, 1.0], // Room Three: Workstation Slate Blue
        3 => [0.30, 0.30, 0.35, 1.0], // Room Four: Unix Terminal Charcoal
        _ => [0.00, 0.47, 0.47, 1.0], // Fallback
    };
    // Classic 1990s CDE Desktop Teal Clear color palette
    [vk::ClearValue {
        color: vk::ClearColorValue {
            float32: background_color,
        },
    }]
}

fn find_pass_ortho_matrix(h: f32, w: f32, wx: f32, wy: f32) -> [f32; 16] {
    // Build the projection layout translation matrix factoring floating window offsets!
    [
        2.0 / w,  0.0,       0.0, 0.0,
        0.0,      2.0 / h,   0.0, 0.0,
        0.0,      0.0,       1.0, 0.0,
       -1.0 + (2.0 * wx / w), -1.0 + (2.0 * wy / h), 0.0, 1.0, // Multiplied coordinate offsets applied here
    ]
}

impl VulkanEngine {
    pub fn init(backend: &WinitGraphicsBackend<GlesRenderer>) -> Self {
        let render_node_or_gpu_device_handle = std::fs::metadata("/dev/dri/renderD128")
            .map(|meta| meta.st_rdev())
            .unwrap_or(0);
        let window_raw = backend.window();
        let window_handle = window_raw.window_handle().unwrap().as_raw();
        let display_handle = window_raw.display_handle().unwrap().as_raw();

        let mut instance_extensions = ash_window::enumerate_required_extensions(display_handle)
            .expect("Failed to locate surface extensions")
            .to_vec();
        instance_extensions.push(khr::surface::NAME.as_ptr());
        let device_extensions = [
            ash::khr::swapchain::NAME.as_ptr(),
            ash::khr::external_memory_fd::NAME.as_ptr(),
            ash::ext::external_memory_dma_buf::NAME.as_ptr(),
        ];

        let (entry, instance, physical_device, device, graphics_queue) = Self::create_core_device(
            &instance_extensions,
            &device_extensions,
            render_node_or_gpu_device_handle,
        );

        let surface = unsafe {
            ash_window::create_surface(&entry, &instance, display_handle, window_handle, None)
                .expect("Failed to create window surface")
        };
        let surface_loader = khr::surface::Instance::new(&entry, &instance);
        let swapchain_loader = khr::swapchain::Device::new(&instance, &device);

        let inner_size = window_raw.inner_size();
        let swapchain_extent = vk::Extent2D {
            width: inner_size.width,
            height: inner_size.height,
        };

        let (
            descriptor_set_layout,
            descriptor_pool,
            descriptor_set,
            texture_sampler,
            render_pass,
            pipeline_layout,
            graphics_pipeline,
        ) = Self::create_pipeline_infrastructure(
            &device,
            swapchain_extent,
            vk::ImageLayout::PRESENT_SRC_KHR,
        );

        // Window presentation-specific attachments logic continues here...
        let swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(2)
            .image_format(vk::Format::B8G8R8A8_UNORM)
            .image_color_space(vk::ColorSpaceKHR::SRGB_NONLINEAR)
            .image_extent(swapchain_extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
            .pre_transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(vk::PresentModeKHR::FIFO);

        let swapchain = unsafe {
            swapchain_loader
                .create_swapchain(&swapchain_create_info, None)
                .unwrap()
        };
        let swapchain_images = unsafe { swapchain_loader.get_swapchain_images(swapchain).unwrap() };
        let swapchain_image_views: Vec<vk::ImageView> = swapchain_images
            .iter()
            .map(|&image| create_default_2d_image_view(&device, image, vk::Format::B8G8R8A8_UNORM))
            .collect();

        let framebuffers: Vec<vk::Framebuffer> = swapchain_image_views
            .iter()
            .map(|&view| {
                let fb_info = vk::FramebufferCreateInfo::default()
                    .render_pass(render_pass)
                    .attachments(std::slice::from_ref(&view))
                    .width(swapchain_extent.width)
                    .height(swapchain_extent.height)
                    .layers(1);
                unsafe { device.create_framebuffer(&fb_info, None).unwrap() }
            })
            .collect();

        let pool_info = vk::CommandPoolCreateInfo::default().queue_family_index(0);
        let command_pool = unsafe { device.create_command_pool(&pool_info, None).unwrap() };
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(framebuffers.len() as u32);
        let command_buffers = unsafe { device.allocate_command_buffers(&alloc_info).unwrap() };

        let semaphore_info = vk::SemaphoreCreateInfo::default();
        let image_available_semaphore =
            unsafe { device.create_semaphore(&semaphore_info, None).unwrap() };
        let render_finished_semaphore =
            unsafe { device.create_semaphore(&semaphore_info, None).unwrap() };
        let in_flight_fence = unsafe {
            device
                .create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )
                .unwrap()
        };

        Self {
            _entry: entry,
            instance,
            device,
            physical_device,
            queue_family_index: 0,
            graphics_queue,
            render_node_or_gpu_device_handle,
            surface_loader,
            surface,
            swapchain_loader,
            swapchain,
            swapchain_images,
            swapchain_image_views,
            swapchain_extent,
            render_frames: vec![],
            current_frame_index: 0,
            render_pass,
            descriptor_set_layout,
            pipeline_layout,
            graphics_pipeline,
            framebuffers,
            command_pool,
            command_buffers,
            image_available_semaphore,
            render_finished_semaphore,
            in_flight_fence,
            texture_sampler,
            descriptor_pool,
            descriptor_set,
            window_textures: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn init_headless(
        width: u32,
        height: u32,
        allocator: &mut GbmAllocator<DrmDeviceFd>,
    ) -> Self {
        let render_node_or_gpu_device_handle = std::fs::metadata("/dev/dri/renderD128")
            .map(|meta| meta.st_rdev())
            .unwrap_or(0);
        let instance_extensions = [
            ash::khr::external_memory_capabilities::NAME.as_ptr(),
            ash::khr::get_physical_device_properties2::NAME.as_ptr(),
        ];
        let device_extensions = [
            ash::khr::external_memory::NAME.as_ptr(),
            ash::khr::external_memory_fd::NAME.as_ptr(),
            ash::ext::external_memory_dma_buf::NAME.as_ptr(),
            ash::ext::image_drm_format_modifier::NAME.as_ptr(),
        ];

        let (entry, instance, physical_device, device, graphics_queue) = Self::create_core_device(
            &instance_extensions,
            &device_extensions,
            render_node_or_gpu_device_handle,
        );

        let swapchain_extent = vk::Extent2D { width, height };

        let (
            descriptor_set_layout,
            descriptor_pool,
            descriptor_set,
            texture_sampler,
            render_pass,
            pipeline_layout,
            graphics_pipeline,
        ) = Self::create_pipeline_infrastructure(
            &device,
            swapchain_extent,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        );

        // Headless manual allocation array loops logic continues here...
        let mut render_frames = Vec::new();
        let mut framebuffers = Vec::new();

        use smithay::backend::allocator::Allocator;
        use std::os::fd::AsRawFd;
        use std::os::fd::IntoRawFd;

        for _ in 0..2 {
            let gbm_buffer = <GbmAllocator<DrmDeviceFd> as Allocator>::create_buffer(
                allocator,
                width,
                height,
                drm_fourcc::DrmFourcc::Xrgb8888,
                &[drm_fourcc::DrmModifier::Linear],
            )
            .expect("Failed to allocate scanout plane via GBM");
            let stride = gbm_buffer.stride();
            let plane_layout = vk::SubresourceLayout {
                row_pitch: stride as vk::DeviceSize,
                ..Default::default()
            };

            let mut drm_modifier_info = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
                .plane_layouts(std::slice::from_ref(&plane_layout));
            let mut external_memory_info = vk::ExternalMemoryImageCreateInfo::default()
                .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

            let image_info = vk::ImageCreateInfo::default()
                .push_next(&mut external_memory_info)
                .push_next(&mut drm_modifier_info)
                .image_type(vk::ImageType::TYPE_2D)
                .extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .format(vk::Format::B8G8R8A8_UNORM)
                .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::SAMPLED);
            let image = unsafe { device.create_image(&image_info, None).unwrap() };

            let dmabuf = gbm_buffer
                .export()
                .expect("Failed to export GBM buffer to Dmabuf");
            let cloned_fd = unsafe {
                std::os::fd::BorrowedFd::borrow_raw(dmabuf.handles().next().unwrap().as_raw_fd())
            }
            .try_clone_to_owned()
            .unwrap()
            .into_raw_fd();
            let img_mem_reqs = unsafe { device.get_image_memory_requirements(image) };

            let mut import_fd_info = vk::ImportMemoryFdInfoKHR::default()
                .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
                .fd(cloned_fd);
            let mut dedicated_alloc_info = vk::MemoryDedicatedAllocateInfo::default().image(image);

            let mem_properties =
                unsafe { instance.get_physical_device_memory_properties(physical_device) };
            let mem_index = (0..mem_properties.memory_type_count)
                .find(|&i| (img_mem_reqs.memory_type_bits & (1 << i)) != 0)
                .unwrap();

            let memory_info = vk::MemoryAllocateInfo::default()
                .allocation_size(img_mem_reqs.size)
                .memory_type_index(mem_index)
                .push_next(&mut import_fd_info)
                .push_next(&mut dedicated_alloc_info);
            let memory = unsafe { device.allocate_memory(&memory_info, None).unwrap() };
            unsafe {
                device.bind_image_memory(image, memory, 0).unwrap();
            }

            let view = create_default_2d_image_view(&device, image, vk::Format::B8G8R8A8_UNORM);
            let framebuffer = unsafe {
                device
                    .create_framebuffer(
                        &vk::FramebufferCreateInfo::default()
                            .render_pass(render_pass)
                            .attachments(std::slice::from_ref(&view))
                            .width(width)
                            .height(height)
                            .layers(1),
                        None,
                    )
                    .unwrap()
            };

            framebuffers.push(framebuffer);
            render_frames.push(GbmRenderFrame {
                gbm_buffer,
                image,
                memory,
                view,
                framebuffer,
            });
        }

        let command_pool = unsafe {
            device
                .create_command_pool(
                    &vk::CommandPoolCreateInfo::default().queue_family_index(0),
                    None,
                )
                .unwrap()
        };
        let command_buffers = unsafe {
            device
                .allocate_command_buffers(
                    &vk::CommandBufferAllocateInfo::default()
                        .command_pool(command_pool)
                        .level(vk::CommandBufferLevel::PRIMARY)
                        .command_buffer_count(2),
                )
                .unwrap()
        };
        let in_flight_fence = unsafe {
            device
                .create_fence(
                    &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                    None,
                )
                .unwrap()
        };

        Self {
            _entry: entry,
            instance,
            device,
            physical_device,
            queue_family_index: 0,
            graphics_queue,
            render_node_or_gpu_device_handle,
            surface_loader: unsafe { std::mem::zeroed() },
            surface: vk::SurfaceKHR::null(),
            swapchain_loader: unsafe { std::mem::zeroed() },
            swapchain: vk::SwapchainKHR::null(),
            swapchain_images: vec![],
            swapchain_image_views: vec![],
            swapchain_extent,
            render_frames,
            current_frame_index: 0,
            render_pass,
            descriptor_set_layout,
            pipeline_layout,
            graphics_pipeline,
            framebuffers,
            command_pool,
            command_buffers,
            image_available_semaphore: vk::Semaphore::null(),
            render_finished_semaphore: vk::Semaphore::null(),
            in_flight_fence,
            texture_sampler,
            descriptor_pool,
            descriptor_set,
            window_textures: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Step 1: Shared Core Instance and Device Setup
    fn create_core_device(
        instance_extensions: &[*const i8],
        device_extensions: &[*const i8],
        target_rdev: u64,
    ) -> (Entry, Instance, vk::PhysicalDevice, Arc<Device>, vk::Queue) {
        let entry = unsafe { Entry::load().expect("Failed to load Vulkan library") };
        let app_info = vk::ApplicationInfo::default()
            .application_name(c"CDE Wayland Compositor")
            .api_version(vk::API_VERSION_1_3);

        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(instance_extensions);

        let instance = unsafe {
            entry
                .create_instance(&create_info, None)
                .expect("Failed to init instance")
        };
        let pdevices = unsafe { instance.enumerate_physical_devices().expect("No GPU found") };

        // Match device with the hardware handle
        let mut physical_device = pdevices[0];
        for pdevice in pdevices {
            let mut properties2 = vk::PhysicalDeviceProperties2::default();
            let mut drm_properties = vk::PhysicalDeviceDrmPropertiesEXT::default();
            properties2 = properties2.push_next(&mut drm_properties);

            unsafe { instance.get_physical_device_properties2(pdevice, &mut properties2) };
            let vulkan_rdev = ((drm_properties.primary_major as u64) << 8)
                | (drm_properties.primary_minor as u64);

            if vulkan_rdev == target_rdev {
                println!(
                    "Binding Vulkan logical instance directly onto discrete VRAM device node indices."
                );
                physical_device = pdevice;
                break;
            }
        }

        let queue_family_index = 0;
        let queue_priorities = [1.0f32];
        let queue_create_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family_index)
            .queue_priorities(&queue_priorities);

        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(std::slice::from_ref(&queue_create_info))
            .enabled_extension_names(device_extensions);

        let device = unsafe {
            instance
                .create_device(physical_device, &device_create_info, None)
                .expect("Failed to instantiate logical Vulkan device")
        };
        let graphics_queue = unsafe { device.get_device_queue(queue_family_index, 0) };

        (
            entry,
            instance,
            physical_device,
            Arc::new(device),
            graphics_queue,
        )
    }

    /// Step 2: Shared Pipeline & Descriptor Infrastructure Setup
    fn create_pipeline_infrastructure(
        device: &Device,
        extent: vk::Extent2D,
        final_image_layout: vk::ImageLayout,
    ) -> (
        vk::DescriptorSetLayout,
        vk::DescriptorPool,
        vk::DescriptorSet,
        vk::Sampler,
        vk::RenderPass,
        vk::PipelineLayout,
        vk::Pipeline,
    ) {
        let sampler_layout_binding = vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT);

        let layout_info = vk::DescriptorSetLayoutCreateInfo::default()
            .bindings(std::slice::from_ref(&sampler_layout_binding));
        let descriptor_set_layout = unsafe {
            device
                .create_descriptor_set_layout(&layout_info, None)
                .unwrap()
        };

        let pool_size = vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(256);
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(256)
            .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
            .pool_sizes(std::slice::from_ref(&pool_size));
        let descriptor_pool = unsafe { device.create_descriptor_pool(&pool_info, None).unwrap() };

        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(descriptor_pool)
            .set_layouts(std::slice::from_ref(&descriptor_set_layout));
        let descriptor_set = unsafe { device.allocate_descriptor_sets(&alloc_info).unwrap()[0] };

        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
            .mipmap_mode(vk::SamplerMipmapMode::LINEAR);
        let texture_sampler = unsafe { device.create_sampler(&sampler_info, None).unwrap() };

        let color_attachment = vk::AttachmentDescription::default()
            .format(vk::Format::B8G8R8A8_UNORM)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(final_image_layout);

        let color_attachment_ref = vk::AttachmentReference::default()
            .attachment(0)
            .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
        let subpass = vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(std::slice::from_ref(&color_attachment_ref));
        let dependency = vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);

        let render_pass_info = vk::RenderPassCreateInfo::default()
            .attachments(std::slice::from_ref(&color_attachment))
            .subpasses(std::slice::from_ref(&subpass))
            .dependencies(std::slice::from_ref(&dependency));
        let render_pass = unsafe { device.create_render_pass(&render_pass_info, None).unwrap() };

        let vert_spv = include_bytes!(concat!(env!("OUT_DIR"), "/cde_border.vert.spv"));
        let frag_spv = include_bytes!(concat!(env!("OUT_DIR"), "/cde_border.frag.spv"));
        let vert_code = ash::util::read_spv(&mut std::io::Cursor::new(&vert_spv[..])).unwrap();
        let frag_code = ash::util::read_spv(&mut std::io::Cursor::new(&frag_spv[..])).unwrap();

        let vert_module = unsafe {
            device
                .create_shader_module(
                    &vk::ShaderModuleCreateInfo::default().code(&vert_code),
                    None,
                )
                .unwrap()
        };
        let frag_module = unsafe {
            device
                .create_shader_module(
                    &vk::ShaderModuleCreateInfo::default().code(&frag_code),
                    None,
                )
                .unwrap()
        };

        let push_constant_range = vk::PushConstantRange::default()
            .stage_flags(vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT)
            .size(std::mem::size_of::<CdePushConstants>() as u32);
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::default()
            .push_constant_ranges(std::slice::from_ref(&push_constant_range))
            .set_layouts(std::slice::from_ref(&descriptor_set_layout));
        let pipeline_layout = unsafe {
            device
                .create_pipeline_layout(&pipeline_layout_info, None)
                .unwrap()
        };

        let shader_stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vert_module)
                .name(c"main"),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(frag_module)
                .name(c"main"),
        ];

        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default();
        let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::Viewport {
            width: extent.width as f32,
            height: extent.height as f32,
            max_depth: 1.0,
            ..Default::default()
        };
        let scissor = vk::Rect2D {
            extent,
            ..Default::default()
        };
        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewports(std::slice::from_ref(&viewport))
            .scissors(std::slice::from_ref(&scissor));
        let rasterizer = vk::PipelineRasterizationStateCreateInfo::default().line_width(1.0);
        let multisample = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let color_blend_attachment = vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD);
        let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
            .attachments(std::slice::from_ref(&color_blend_attachment));

        let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stages)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_assembly)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterizer)
            .multisample_state(&multisample)
            .color_blend_state(&color_blending)
            .layout(pipeline_layout)
            .render_pass(render_pass);
        let graphics_pipeline = unsafe {
            device
                .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
                .expect("Pipeline build failed")[0]
        };

        unsafe {
            device.destroy_shader_module(vert_module, None);
            device.destroy_shader_module(frag_module, None);
        }

        (
            descriptor_set_layout,
            descriptor_pool,
            descriptor_set,
            texture_sampler,
            render_pass,
            pipeline_layout,
            graphics_pipeline,
        )
    }

    /// Helper utility to allocate a descriptor set and link it to an image view.
    fn allocate_and_write_descriptor_set(&self, image_view: vk::ImageView) -> vk::DescriptorSet {
        unsafe {
            let alloc_info = vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(self.descriptor_pool)
                .set_layouts(std::slice::from_ref(&self.descriptor_set_layout));
            let descriptor_set = self.device.allocate_descriptor_sets(&alloc_info).unwrap()[0];

            let image_info = vk::DescriptorImageInfo::default()
                .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .image_view(image_view)
                .sampler(self.texture_sampler);

            let descriptor_write = vk::WriteDescriptorSet::default()
                .dst_set(descriptor_set)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(std::slice::from_ref(&image_info));

            self.device.update_descriptor_sets(&[descriptor_write], &[]);
            descriptor_set
        }
    }

    /// Helper to queries requirements, allocates device-local GPU memory, and binds it to an image.
    fn allocate_and_bind_image_memory(&self, image: vk::Image,
                                      import_fd: Option<i32>,
                                      required_properties: vk::MemoryPropertyFlags) -> vk::DeviceMemory {
        unsafe {
            // Allocate device memory and bind to texture_image...
            // Look up the physical memory requirements for the destination GPU texture image
            let img_mem_reqs = self.device.get_image_memory_requirements(image);
            let img_mem_index = self.find_memory_type(
                img_mem_reqs.memory_type_bits,
                required_properties,
            );
            let mut img_alloc_info = vk::MemoryAllocateInfo::default()
                .allocation_size(img_mem_reqs.size)
                .memory_type_index(img_mem_index);

            let mut dedicated_alloc_info = vk::MemoryDedicatedAllocateInfo::default().image(image);
            img_alloc_info = img_alloc_info.push_next(&mut dedicated_alloc_info);

            let memory = match import_fd {
                Some(fd) => {
                    let mut import_fd_info = vk::ImportMemoryFdInfoKHR::default()
                        .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
                        .fd(fd);
                    img_alloc_info = img_alloc_info.push_next(&mut import_fd_info);
                    self.device.allocate_memory(&img_alloc_info, None).unwrap()
                }
                None => {
                    self.device.allocate_memory(&img_alloc_info, None).unwrap()
                }
            };

            self.device.bind_image_memory(image, memory, 0).unwrap();

            memory
        }
    }

    /// Submits a recorded command buffer to the graphics queue and presents the target swapchain image index.
    fn submit_and_present_frame(&self, cmd_buf: vk::CommandBuffer, image_index: u32) {
        unsafe {
            let wait_semaphores = [self.image_available_semaphore];
            let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
            let signal_semaphores = [self.render_finished_semaphore];

            let submit_info = vk::SubmitInfo::default()
                .wait_semaphores(&wait_semaphores)
                .wait_dst_stage_mask(&wait_stages)
                .command_buffers(std::slice::from_ref(&cmd_buf))
                .signal_semaphores(&signal_semaphores);

            self.device
                .queue_submit(
                    self.graphics_queue,
                    std::slice::from_ref(&submit_info),
                    self.in_flight_fence,
                )
                .unwrap();

            let present_info = vk::PresentInfoKHR::default()
                .wait_semaphores(&signal_semaphores)
                .swapchains(std::slice::from_ref(&self.swapchain))
                .image_indices(std::slice::from_ref(&image_index));

            let present_result = self.swapchain_loader.queue_present(self.graphics_queue, &present_info);
            match present_result {
                Ok(_) | Err(vk::Result::SUBOPTIMAL_KHR) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {}
                Err(e) => panic!("Fatal presentation lane submission failure: {:?}", e),
            }
        }
    }

    /// Helper utility to find hardware supported host memory type pools dynamically
    fn find_memory_type(&self, type_filter: u32, properties: vk::MemoryPropertyFlags) -> u32 {
        let mem_properties = unsafe {
            self.instance
                .get_physical_device_memory_properties(self.physical_device)
        };
        // Slice the memory types array up to the valid count reported by Vulkan
        let valid_types = &mem_properties.memory_types[..mem_properties.memory_type_count as usize];
        valid_types
            .iter()
            .enumerate()
            .map(|(i, mem_type)| (i as u32, mem_type))
            .find(|&(index, mem_type)| {
                let is_required_type = (type_filter & (1 << index)) != 0;
                let has_required_properties = mem_type.property_flags.contains(properties);

                is_required_type && has_required_properties
            }).map(|(index, _)| index)
            .expect("Failed to find suitable host visible memory type pointer configuration")
    }

    /// Helper utility to render a flat or procedurally chiseled UI frame (Lock box, icons, panels).
    fn draw_procedural_ui_element(
        &self,
        cmd_buf: vk::CommandBuffer,
        screen_h: f32,
        screen_w: f32,
        rect: (f32, f32, f32, f32), // (x, y, width, height)
        state_flag: u32,
    ) {
        let (x, y, w, h) = rect;
        let ortho_matrix = find_pass_ortho_matrix(screen_h, screen_w, x, y);
        let push_constants = CdePushConstants::new_ui_element(ortho_matrix, [w, h], state_flag);

        self.draw_element(cmd_buf, self.descriptor_set, push_constants);
    }

    pub fn draw_element(
        &self,
        cmd_buf: vk::CommandBuffer,
        descriptor_set: vk::DescriptorSet,
        push_constants: CdePushConstants,
    ) {
        unsafe {
            self.device.cmd_bind_pipeline(
                cmd_buf,
                vk::PipelineBindPoint::GRAPHICS,
                self.graphics_pipeline,
            );

            // Bind the descriptor set containing your terminal frame image sampler to binding 0!
            self.device.cmd_bind_descriptor_sets(
                cmd_buf,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,                                     // Global descriptor set indexing mapping
                std::slice::from_ref(&descriptor_set), // XXX Use the dynamic set passed in.
                &[],
            );

            let constants_ptr = &push_constants as *const CdePushConstants as *const u8;
            let constants_slice =
                std::slice::from_raw_parts(constants_ptr, std::mem::size_of::<CdePushConstants>());
            self.device.cmd_push_constants(
                cmd_buf,
                self.pipeline_layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                constants_slice,
            );
            // Draw a full procedural screen quad (6 vertices forming 2 triangles)
            self.device.cmd_draw(cmd_buf, 6, 1, 0, 0);
        }
    }

    pub fn draw_frame(
        &mut self,
        open_windows: &[WindowContext],
        current_grab: crate::GrabState,
        active_workspace: u32,
        is_session_locked: bool,
        password_length: u32,
        active_menu: Option<ActiveMenu>,
    ) -> Option<&GbmBuffer> {
        unsafe {
            // Wait for previous GPU frames to complete
            self.device
                .wait_for_fences(std::slice::from_ref(&self.in_flight_fence), true, u64::MAX)
                .unwrap();

            // Extract dimensions for the entire frame loop
            let screen_w = self.swapchain_extent.width as f32;
            let screen_h = self.swapchain_extent.height as f32;

            let (image_index, target_framebuffer) = if self.swapchain != vk::SwapchainKHR::null() {
                // WINDOWED WINIT PATH: Acquire next frame out of the live window engine queue layout
                // Handle OUT_OF_DATE errors when acquiring the next swapchain image slot
                let acquire_result = self.swapchain_loader.acquire_next_image(
                    self.swapchain,
                    u64::MAX,
                    self.image_available_semaphore,
                    vk::Fence::null(),
                );
                // Acquire next swapchain slot index
                let idx = match acquire_result {
                    Ok((index, _)) => index,
                    Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                        // Window shifted size mid-cycle! Skip this frame and bail
                        println!("Swapchain out of date during image acquisition. Skipping frame...");
                        return None;
                    }
                    Err(e) => panic!("Fatal swapchain image acquire failure: {:?}", e),
                };

                (idx as usize, self.framebuffers[idx as usize])
            } else {
                // BARE-METAL HARDWARE PATH: Advance manual frame counter through our internal double-buffer array
                self.current_frame_index = (self.current_frame_index + 1) % self.render_frames.len();
                (self.current_frame_index, self.render_frames[self.current_frame_index].framebuffer)
            };

            self.device
                .reset_fences(std::slice::from_ref(&self.in_flight_fence))
                .unwrap();

            let cmd_buf = self.command_buffers[image_index as usize];
            self.device
                .reset_command_buffer(cmd_buf, vk::CommandBufferResetFlags::empty())
                .unwrap();
            self.device
                .begin_command_buffer(cmd_buf, &vk::CommandBufferBeginInfo::default())
                .unwrap();

            let clear_values = workspace_clear_colour(active_workspace);
            let render_pass_begin_info = vk::RenderPassBeginInfo::default()
                .render_pass(self.render_pass)
                .framebuffer(target_framebuffer) // Dynamically targets the explicit Winit or GBM framebuffer calculated above
                //.framebuffer(self.framebuffers[image_index as usize])
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent: self.swapchain_extent,
                })
                .clear_values(&clear_values);
            self.device.cmd_begin_render_pass(
                cmd_buf,
                &render_pass_begin_info,
                vk::SubpassContents::INLINE,
            );

            if is_session_locked {
                // ----------------===========================================
                // CDE Lock screen draw pass
                // ----------------===========================================
                // Calculate Exact Orthographic Coordinates to Center the Lock Box
                let box_x = (screen_w - CDE_LOCK_WIDTH) / 2.0;
                let box_y = (screen_h - CDE_LOCK_HEIGHT) / 2.0;
                self.draw_procedural_ui_element(
                    cmd_buf,
                    screen_h,
                    screen_w,
                    (box_x, box_y, crate::CDE_LOCK_WIDTH, crate::CDE_LOCK_HEIGHT),
                    4u32 | (password_length << 4),
                );

                // Close render pass execution pipelines
                self.device.cmd_end_render_pass(cmd_buf);
                self.device.end_command_buffer(cmd_buf).unwrap();

                // Submit primitives directly down the hardware queues
                self.submit_and_present_frame(cmd_buf, image_index as u32);
                return None; // Ensure background windows aren't drawn!
            }

            // Draw our retro 3D chiseled element container
            // Re-enable drawing your window box component inside the active render pass!
            // This reads uploaded client texture and maps it onto the orthographic screen geometry.
            // Access the active texture table to draw each window box individually
            let textures = self.window_textures.lock().unwrap();

            for (index, window) in open_windows.iter().enumerate() {
                // Skip drawing entirely if the window belongs to a different desktop pane
                if window.workspace != active_workspace {
                    continue;
                }

                // Evaluate focus - If this window is the top-most layout inside our open vector stack, it is focused!
                let is_focused: u32 = if index == open_windows.len() - 1 { 1 } else { 0 };

                // Evaluate active interactive dragging status.
                let is_dragging: u32 = match current_grab {
                    crate::GrabState::Dragging { window_index, .. } => {
                        if window_index == index { 1 } else { 0 }
                    }
                    _ => 0,
                };

                // Evaluate active interactive pressing status.
                let is_pressed: u32 = match current_grab {
                    crate::GrabState::ButtonPressed { window_index, button_type } => {
                        if window_index == index { button_type } else { 0 }
                    }
                    _ => 0,
                };

                // Extract Application Name and convert to an integer array buffer
                let surface = window.toplevel.wl_surface();
                let title_buffer: Vec<u32> = extract_surface_title(surface)
                    .chars().map(|ch| ch as u32).collect();

                if window.is_minimized {
                    // Render a blocky, compact 64x64 gray chiseled frame box directly onto the desktop layout
                    self.draw_procedural_ui_element(
                        cmd_buf,
                        screen_h,
                        screen_w,
                        (window.x, window.y, CDE_ICON_WIDTH, CDE_ICON_HEIGHT),
                        0u32, // State token 0 signals a traditional flat bevel frame component
                    );
                    continue; // Bypasses live app texture overlay generation passes completely!
                }

                smithay::wayland::compositor::with_states(surface, |states| {
                    let mut guard = states.cached_state.get::<smithay::wayland::compositor::SurfaceAttributes>();

                    if let Some(smithay::wayland::compositor::BufferAssignment::NewBuffer(_wl_buffer)) = &guard.current().buffer {
                        // Look up the dynamic texture layer allocated for this individual client window surface frame
                        //if let Some(tex) = textures.get(&wl_buffer.id()) {
                        if let Some(tex) = textures.get(&window.id) {
                            if window.width > 0.0 && window.height > 0.0 {
                                // ----------------===========================================
                                // Draw the custom retro outer frame / titlebar container
                                // ----------------===========================================

                                // We pass index 0 (or a dedicated static slot) of our descriptor sets array
                                // to draw a fallback texture or flat color palette matching your CDE theme.
                                // This acts as the thick chiseled outer border.
                                //let fallback_frame_set = self.descriptor_sets[0];

                                // Build the projection layout translation matrix factoring floating window offsets!
                                let pass1_ortho_matrix = find_pass_ortho_matrix(screen_h, screen_w, window.x, window.y);
                                let title_chars = {
                                    let mut tmp = [0u32; 16];
                                    let len = title_buffer.len().min(16);
                                    tmp[..len].copy_from_slice(&title_buffer[..len]);
                                    tmp
                                };
                                let pass1_push_constants = CdePushConstants {
                                    mvp: pass1_ortho_matrix,
                                    element_size: [window.width, window.height],
                                    is_focused,
                                    is_dragging,
                                    is_pressed,
                                    title_chars: title_chars,
                                };
                                // We pass the global `self.descriptor_set` handle layout structure.
                                // This provides a fallback texture bind context block, signaling to the
                                // fragment shader to render procedural 3D chiseled gray border accents.
                                self.draw_element(
                                    cmd_buf,
                                    self.descriptor_set, // Uses your root un-mapped initialization set
                                    pass1_push_constants,
                                );

                                // ----------------===========================================
                                // Draw live application inside texture interior
                                // ------------------------------------------------===========
                                // Offset the coordinates inward to nested borders limits
                                let client_x = window.x + CDE_BORDER_WIDTH;

                                // Instead of adding CDE_BORDER_WIDTH, we add CDE_TITLEBAR_HEIGHT so the texture
                                // renders cleanly beneath your blue CDE focus header panel strips.
                                let client_y = window.y + CDE_TITLEBAR_HEIGHT;
                                //let client_y = window.y + CDE_BORDER_WIDTH; // Sits neatly above the bottom border strip

                                // Deduct side borders and top titlebar padding from total rendering envelope space
                                let client_w = window.width - (CDE_BORDER_WIDTH * 2.0);
                                let client_h = window.height - (CDE_BORDER_WIDTH + CDE_TITLEBAR_HEIGHT);
                                // Calculate projection mapping aligned exactly with inner client viewport offsets
                                let pass2_ortho_matrix = find_pass_ortho_matrix(screen_h, screen_w, client_x, client_y);

                                let pass2_push_constants = CdePushConstants {
                                    mvp: pass2_ortho_matrix,
                                    element_size: [client_w, client_h],
                                    is_focused: 3u32, // Distinct state flag signaling raw app texture mapping pass
                                    is_dragging,
                                    is_pressed,
                                    title_chars: [0u32; 16], // Not required for inner canvas layer
                                };
                                // Render the actual window client context inside the framed boundaries,
                                // passing its own uniquely allocated descriptor set.
                                // Draw this explicit window box using its cached texture map descriptor!
                                self.draw_element(
                                    cmd_buf,
                                    tex.descriptor_set, // Binds this unique client container image layout
                                    pass2_push_constants,
                                );
                            }
                        }
                    }
                });
            }

            // Render CDE front panel desktop dock
            {
                // Calculate bottom center positioning matching the current monitor resolution
                let panel_x = (screen_w - CDE_PANEL_WIDTH) / 2.0;
                let panel_y = screen_h - CDE_PANEL_HEIGHT - 10.0; // Anchored 10 pixels above the absolute screen bottom
                // We encode the workspace selection into the higher bits of our integer flag!
                // is_focused = 2 tells the shader it's the panel.
                // Adding (active_workspace << 4) bundles the chosen desk index cleanly without extra push constant slots!
                // We pass `2` for the `is_focused` parameter. This special flag tells our fragment
                // shader: "This element is the Front Panel Dock—render interior button rows!"
                self.draw_procedural_ui_element(
                    cmd_buf,
                    screen_h, screen_w,
                    (panel_x, panel_y, CDE_PANEL_WIDTH, CDE_PANEL_HEIGHT),
                    2u32 | (active_workspace << 4), // Encodes (2 | active_workspace << 4)
                );

                // Live FRONT PANEL Clock Text Overlay Pipeline
                // Position the clock text lane on the left side of the Switcher matrix
                let clock_w = 60.0;
                let clock_h = 16.0;
                let clock_x = panel_x + (CDE_PANEL_WIDTH / 2.0) - 130.0; // Pinned neatly to the left of the switcher
                let clock_y = panel_y + ((CDE_PANEL_HEIGHT - clock_h) / 2.0);

                let time_str = chrono::Local::now().format("%H:%M:%S").to_string();
                let mut clock_chars = [0u32; 16];
                for (i, ch) in time_str.chars().take(16).enumerate() {
                    clock_chars[i] = ch as u32;
                }

                let pass_matrix = find_pass_ortho_matrix(screen_h, screen_w, clock_x, clock_y);
                let clock_push_constants = CdePushConstants {
                    mvp: pass_matrix,
                    element_size: [clock_w, clock_h],
                    is_focused: 6u32, // Tell the frag shader to render a clock
                    is_dragging: 0u32,
                    is_pressed: 0u32,
                    title_chars: clock_chars, // Feed character tokens straight to shader
                };

                self.draw_element(cmd_buf, self.descriptor_set, clock_push_constants);
            }

            // Render interactive CDE procedural dropdown menu
            if let Some(menu) = active_menu {
                // If window_id == 0, we add (1 << 4) to shift state flag indices cleanly down the shader pipeline.
                let packed_menu_flag = if menu.window_id == 0 { 5u32 | (1u32 << 4) } else { 5u32 };

                // is_focused = 5 signals your fragment shader to render a retro chiseled
                // popup container overlay featuring 4 internal highlighted text item rows.
                self.draw_procedural_ui_element(
                    cmd_buf,
                    screen_h,
                    screen_w,
                    (menu.x, menu.y, menu.width, menu.height),
                    packed_menu_flag,
                );
            }

            self.device.cmd_end_render_pass(cmd_buf);
            self.device.end_command_buffer(cmd_buf).unwrap();

            if self.swapchain != vk::SwapchainKHR::null() {
                // Winit path presents straight downstream into the windowing surface frame queue
                self.submit_and_present_frame(cmd_buf, image_index as u32);
                None
            } else {
                // Bare-metal hardware path pushes recorded commands to the queue and returns the active GBM texture handle
                let submit_info = vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cmd_buf));
                self.device.queue_submit(self.graphics_queue, std::slice::from_ref(&submit_info), self.in_flight_fence).unwrap();
                Some(&self.render_frames[image_index].gbm_buffer)
            }
            //self.submit_and_present_frame(cmd_buf, image_index);
        }
    }

    fn execute_one_time_commands<F>(&self, handler: F)
    where
        F: FnOnce(vk::CommandBuffer),
    {
        unsafe {
            let alloc_info = vk::CommandBufferAllocateInfo::default()
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_pool(self.command_pool)
                .command_buffer_count(1);
            let mut buffers = self.device.allocate_command_buffers(&alloc_info).unwrap();
            let cmd_buf = buffers.remove(0);

            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device
                .begin_command_buffer(cmd_buf, &begin_info)
                .unwrap();

            handler(cmd_buf);

            self.device.end_command_buffer(cmd_buf).unwrap();

//            let fence_info = vk::FenceCreateInfo::default();
//            let transfer_fence = self.device.create_fence(&fence_info, None).unwrap();

            let submit_info =
                vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&cmd_buf));

            self.device
                .queue_submit(
                    self.graphics_queue,
                    std::slice::from_ref(&submit_info),
//                    transfer_fence, // Signal this fence when copy finishes
                    vk::Fence::null(),
                )
                .unwrap();
            // XXX use fence instead!
            //self.device.queue_wait_idle(self.graphics_queue).unwrap(); // Synchronize before cleanup
            // Wait on the specific transfer fence instead of stalling the global hardware queue!
            //self.device.wait_for_fences(&[transfer_fence], true, u64::MAX).unwrap();
            //self.device.destroy_fence(transfer_fence, None);
            // Instead of blocking with a fence here, we wait for the queue to finish processing.
            // For a production compositor, manage these staging buffers asynchronously
            // over multiple frames, but this unblocks the event loop immediately.
            self.device.queue_wait_idle(self.graphics_queue).unwrap();

            self.device
                .free_command_buffers(self.command_pool, &[cmd_buf]);
        }
    }

    /// Uploads an app pixel slice into a Vulkan Image sampler and layers it inside your CDE frame boundaries
    pub fn process_client_pixels(&self, window_id: u32, pixels: &[u8], width: u32, height: u32) {
        // Assuming standard 32-bit RGBA/BGMA formatting
        // Calculate standard row stride matching the 32-bit layout footprint (700 * 4 = 2800 bytes)
        let stride = width * 4;
        let buffer_size = (stride * height) as vk::DeviceSize;
        if pixels.len() < buffer_size as usize { return; } // Boundary guard

        println!("DEBUG: First few bytes: {:?}", &pixels[0..16]);

        unsafe {
            // Allocate a temporary Staging Buffer on the CPU
            let staging_info = vk::BufferCreateInfo::default()
                .size(buffer_size)
                .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                .sharing_mode(vk::SharingMode::EXCLUSIVE);
            let staging_buffer = self.device.create_buffer(&staging_info, None).unwrap();

            // Look up memory requirements and bind allocation mapping
            // XXX dedup with self.allocate_and_bind_image_memory(staging_buffer) somehow?
            let mem_reqs = self.device.get_buffer_memory_requirements(staging_buffer);
            let mem_index = self.find_memory_type(
                mem_reqs.memory_type_bits,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            );
            let alloc_info = vk::MemoryAllocateInfo::default()
                .allocation_size(mem_reqs.size)
                .memory_type_index(mem_index);
            let staging_memory = self.device.allocate_memory(&alloc_info, None).unwrap();
            self.device
                .bind_buffer_memory(staging_buffer, staging_memory, 0)
                .unwrap();
            // XXX

            // Map the memory and copy the raw client app bytes directly onto the staging area
            let data_ptr = self
                .device
                .map_memory(staging_memory, 0, buffer_size, vk::MemoryMapFlags::empty())
                .unwrap();

            std::ptr::copy_nonoverlapping(
                pixels.as_ptr(),
                data_ptr as *mut u8,
                buffer_size.try_into().unwrap(),
            );
            self.device.unmap_memory(staging_memory);

            let mut textures = self.window_textures.lock().unwrap();
            let texture_needs_recreation = textures
                .get(&window_id)
                .map_or(true, |tex| !tex.matches_specs(width, height, false));


            if texture_needs_recreation {
                // Dimensions changed or texture is unallocated: Perform clean reallocation pass
                self.device.queue_wait_idle(self.graphics_queue).unwrap();
                textures.remove(&window_id);

                // Create new destination GPU Device-Local Image Texture
                let image_info = vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .extent(vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .format(vk::Format::B8G8R8A8_UNORM) // Standard Wayland SHM format mapping
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .initial_layout(vk::ImageLayout::UNDEFINED)
                    .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE)
                    .samples(vk::SampleCountFlags::TYPE_1);
                let new_image = self.device.create_image(&image_info, None).unwrap();
                let new_memory = self.allocate_and_bind_image_memory(new_image, None,
                                                                     vk::MemoryPropertyFlags::DEVICE_LOCAL); // Stored strictly in fast GPU VRAM pools

                let new_view = create_default_2d_image_view(&self.device, new_image, vk::Format::B8G8R8A8_UNORM);
                let new_descriptor_set = self.allocate_and_write_descriptor_set(new_view);

                textures.insert(
                    window_id,
                    WindowTexture {
                        device: self.device.clone(),
                        descriptor_pool: self.descriptor_pool,
                        image: new_image,
                        memory: new_memory,
                        view: new_view,
                        descriptor_set: new_descriptor_set,
                        width,
                        height,
                        is_dmabuf: false,
                    },
                );
            }

            let target = textures.get(&window_id).unwrap();

            // Record a copy command into a one-time command stream to transfer the buffer to the image
            // self.device.cmd_copy_buffer_to_image(cmd_buf, staging_buffer, texture_image, ...);
            self.execute_one_time_commands(|cmd_buf| {
                // Add a global memory barrier to ensure CPU cache writes are completely visible
                // to the GPU before any image copy or layout transitions execute!
                let memory_barrier = vk::MemoryBarrier::default()
                    .src_access_mask(vk::AccessFlags::HOST_WRITE)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ);

                self.device.cmd_pipeline_barrier(
                    cmd_buf,
                    vk::PipelineStageFlags::HOST,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    std::slice::from_ref(&memory_barrier),
                    &[],
                    &[],
                );

                // Transition Image Layout from UNDEFINED to TRANSFER_DST_OPTIMAL
                let barrier_to_dst = vk::ImageMemoryBarrier::default()
                    //.old_layout(vk::ImageLayout::UNDEFINED)
                    .old_layout(if texture_needs_recreation {
                        vk::ImageLayout::UNDEFINED
                    } else {
                        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
                    })
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(target.image)
                    .subresource_range(default_color_subresource_range())
                    //.src_access_mask(vk::AccessFlags::empty())
                    .src_access_mask(if texture_needs_recreation {
                        vk::AccessFlags::empty()
                    } else {
                        vk::AccessFlags::SHADER_READ
                    })
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);

                self.device.cmd_pipeline_barrier(
                    cmd_buf,
                    if texture_needs_recreation {
                        vk::PipelineStageFlags::TOP_OF_PIPE
                    } else {
                        vk::PipelineStageFlags::FRAGMENT_SHADER
                    },
                    //vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    std::slice::from_ref(&barrier_to_dst),
                );

                // Copy Staging Buffer into our GPU VRAM Image
                let image_subresource = vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1);
                let region = vk::BufferImageCopy::default()
                    .image_subresource(image_subresource)
                    .image_extent(vk::Extent3D {
                        width,
                        height,
                        depth: 1,
                    });

                self.device.cmd_copy_buffer_to_image(
                    cmd_buf,
                    staging_buffer,
                    target.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    std::slice::from_ref(&region),
                );
                // Transition Image Layout from TRANSFER_DST_OPTIMAL to SHADER_READ_ONLY_OPTIMAL
                let barrier_to_readable = vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(target.image)
                    .subresource_range(default_color_subresource_range())
                    .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                    .dst_access_mask(vk::AccessFlags::SHADER_READ);

                self.device.cmd_pipeline_barrier(
                    cmd_buf,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    // Switch destination flag to ALL_GRAPHICS to sync with the main rendering pipeline
                    //vk::PipelineStageFlags::ALL_GRAPHICS,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    std::slice::from_ref(&barrier_to_readable),
                );
            });

            // Clean up temporary staging structures once your copy signals complete
            self.device.destroy_buffer(staging_buffer, None);
            self.device.free_memory(staging_memory, None);
        }
    }

    pub fn import_dmabuf_as_texture(
        &self,
        window_id: u32,
        fd: BorrowedFd<'_>,
        width: u32,
        height: u32,
        stride: u32,
        fourcc: drm_fourcc::DrmFourcc,
    ) {
        unsafe {
            let mut textures = self.window_textures.lock().unwrap();
            // Check if this window already has an identical texture allocated to avoid overhead
            // Reusing hardware dmabuf handles across frames is managed transparently
            // by the kernel plane layer driver! We can skip allocation entirely.
            if textures.get(&window_id).map_or(false, |tex| tex.matches_specs(width, height, true)) {
                return;
            }

            // If dimensions changed or it's a new window, perform a localized clean allocation pass
            self.device.queue_wait_idle(self.graphics_queue).unwrap();

            // Clean up old window texture resources if they exist
            textures.remove(&window_id);

            // Tweak the Vulkan format dynamically based on what the client allocated
            let vulkan_format = map_drm_fourcc_to_vulkan(fourcc);

            // Structure the layout mapping for Plane 0 explicitly
            let plane_layout = vk::SubresourceLayout {
                offset: 0,
                size: 0, // Driver calculates this based on width/height/stride
                row_pitch: stride as vk::DeviceSize,
                array_pitch: 0,
                depth_pitch: 0,
            };

            // Link the plane layout to an explicit DRM format modifier structure
            let mut drm_modifier_info = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
                .drm_format_modifier(0) // 0 maps to DRM_FORMAT_MOD_LINEAR matching main.rs
                .plane_layouts(std::slice::from_ref(&plane_layout));

            // Configure the External Image properties
            let mut external_memory_info = vk::ExternalMemoryImageCreateInfo::default()
                .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

            // Chain both external configurations into the root image creation structure
            let image_info = vk::ImageCreateInfo::default()
                .push_next(&mut external_memory_info)
                .push_next(&mut drm_modifier_info) // Tells driver how to decode the rows
                .image_type(vk::ImageType::TYPE_2D)
                .extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                //.format(vk::Format::B8G8R8A8_UNORM)
                .format(vulkan_format) // Use the dynamic variable
                // Must use DRM_FORMAT_MODIFIER_EXT when importing active dmabufs
                .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .usage(vk::ImageUsageFlags::SAMPLED)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .samples(vk::SampleCountFlags::TYPE_1);

            ///////
            // Duplicate the file descriptor so Vulkan can take complete ownership of it
            let cloned_fd = fd.try_clone_to_owned()
                .expect("Failed to duplicate incoming DMABUF file descriptor handle");
            // We use into_raw_fd() to intentionally leak ownership to the Vulkan driver layer,
            // preventing Rust's Drop checker from closing it when this scope ends.
            use std::os::fd::IntoRawFd;
            let raw_fd = cloned_fd.into_raw_fd();
            /////

            let imported_image = self.device.create_image(&image_info, None).unwrap();
            let imported_memory = self.allocate_and_bind_image_memory(imported_image, Some(raw_fd), vk::MemoryPropertyFlags::empty());

            // Transition barrier for debug as the first frame seems to freeze until it gets
            // resized???
            self.execute_one_time_commands(|cmd_buf| {
                let barrier_to_readable = vk::ImageMemoryBarrier::default()
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .image(imported_image)
                    .subresource_range(default_color_subresource_range())
                    .src_access_mask(vk::AccessFlags::empty())
                    .dst_access_mask(vk::AccessFlags::SHADER_READ);

                self.device.cmd_pipeline_barrier(
                    cmd_buf,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    std::slice::from_ref(&barrier_to_readable),
                );
            });

            let imported_view = create_default_2d_image_view(&self.device, imported_image, vulkan_format);
            // Dynamically allocate a separate descriptor set for this window:
            let new_descriptor_set = self.allocate_and_write_descriptor_set(imported_view);

            // Cache the newly allocated window structure metrics
            textures.insert(
                window_id,
                WindowTexture {
                    device: self.device.clone(),
                    descriptor_pool: self.descriptor_pool,
                    image: imported_image,
                    memory: imported_memory,
                    view: imported_view,
                    descriptor_set: new_descriptor_set,
                    width,
                    height,
                    is_dmabuf: true,
                },
            );
        }
    }
}

/// Helper utility to instantiate a standard 2D Color Image View wrapper.
fn create_default_2d_image_view(device: &Device, image: vk::Image, format: vk::Format) -> vk::ImageView {
    let view_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .subresource_range(default_color_subresource_range());
    unsafe { device.create_image_view(&view_info, None).unwrap() }
}

const fn default_color_subresource_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}

fn map_drm_fourcc_to_vulkan(fourcc: drm_fourcc::DrmFourcc) -> vk::Format {
    match fourcc {
        drm_fourcc::DrmFourcc::Argb8888 | drm_fourcc::DrmFourcc::Xrgb8888 => {
            vk::Format::B8G8R8A8_UNORM
        }
        drm_fourcc::DrmFourcc::Bgrx8888 => vk::Format::R8G8B8A8_UNORM, // Flipped to correct channels
        _ => vk::Format::B8G8R8A8_UNORM, // Default fallback
    }
}

fn extract_surface_title(surface: &wayland_server::protocol::wl_surface::WlSurface) -> String {
    smithay::wayland::compositor::with_states(surface, |states| {
        states
            .data_map
            .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
            .and_then(|data| data.lock().unwrap().title.clone())
    })
    .unwrap_or_else(|| "Application".to_string())
}
