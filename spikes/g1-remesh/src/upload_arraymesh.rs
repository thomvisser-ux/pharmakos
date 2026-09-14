// SPDX-FileCopyrightText: 2026 Pharmakos contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//! **Path A — ArrayMesh + MeshInstance3D.**
//!
//! Presentation side of the wall. Idiomatic and inspectable: every chunk is a
//! real node with a real `ArrayMesh` resource you can click on in the remote
//! scene tree. The cost is a trip through Godot's `Variant`/Packed-array
//! marshalling and a **brand new mesh resource on every remesh** — the resource
//! churn path B exists to avoid.

use godot::classes::mesh::{ArrayType, PrimitiveType};
use godot::classes::{ArrayMesh, Material, MeshInstance3D, Node3D};
use godot::prelude::*;

use crate::greedy::MeshBuf;
use crate::world::{CHUNK_COUNT, chunk_origin};

pub struct ArrayMeshPath {
    holder: Gd<Node3D>,
    material: Gd<Material>,
    instances: Vec<Option<Gd<MeshInstance3D>>>,
    pub resources_created: u64,
}

/// Build the `[ArrayType::MAX]` surface array. Shared by both paths.
pub fn build_arrays(buf: &MeshBuf) -> VarArray {
    let n = buf.pos.len();

    let mut verts = PackedVector3Array::new();
    verts.resize(n);
    {
        let s = verts.as_mut_slice();
        for (i, p) in buf.pos.iter().enumerate() {
            s[i] = Vector3::new(p[0], p[1], p[2]);
        }
    }

    let mut cols = PackedColorArray::new();
    cols.resize(n);
    {
        let s = cols.as_mut_slice();
        for (i, c) in buf.col.iter().enumerate() {
            s[i] = Color::from_rgba(c[0], c[1], c[2], c[3]);
        }
    }

    let mut idx = PackedInt32Array::new();
    idx.resize(buf.idx.len());
    idx.as_mut_slice().copy_from_slice(&buf.idx);

    let mut arrays = VarArray::new();
    arrays.resize(ArrayType::MAX.ord() as usize, &Variant::nil());
    arrays.set(ArrayType::VERTEX.ord() as usize, &verts.to_variant());
    arrays.set(ArrayType::COLOR.ord() as usize, &cols.to_variant());
    arrays.set(ArrayType::INDEX.ord() as usize, &idx.to_variant());
    arrays
}

impl ArrayMeshPath {
    pub fn new(holder: Gd<Node3D>, material: Gd<Material>) -> Self {
        Self {
            holder,
            material,
            instances: (0..CHUNK_COUNT).map(|_| None).collect(),
            resources_created: 0,
        }
    }

    /// Upload one chunk surface. Returns the number of bytes actually written
    /// across the FFI, which for path A is always the whole surface — 28 B per
    /// vertex plus 4 B per index — because there is no in-place branch: every
    /// remesh builds a fresh `ArrayMesh`. (Path B's figure varies by branch;
    /// see `upload_rs::RsPath::upload`.)
    pub fn upload(&mut self, ci: usize, buf: &MeshBuf) -> usize {
        let bytes = buf.bytes();

        if buf.is_empty() {
            if let Some(mi) = self.instances[ci].as_mut() {
                mi.set_visible(false);
            }
            return 0;
        }

        let mut mesh = ArrayMesh::new_gd();
        mesh.add_surface_from_arrays(PrimitiveType::TRIANGLES, &build_arrays(buf));
        mesh.surface_set_material(0, &self.material);
        mesh.set_custom_aabb(Aabb::new(Vector3::ZERO, Vector3::splat(32.0)));
        self.resources_created += 1;

        match self.instances[ci].as_mut() {
            Some(mi) => {
                mi.set_mesh(&mesh);
                mi.set_visible(true);
            }
            None => {
                let (ox, oy, oz) = chunk_origin(ci);
                let mut mi = MeshInstance3D::new_alloc();
                mi.set_name(&format!("chunk_{ci}"));
                mi.set_position(Vector3::new(ox as f32, oy as f32, oz as f32));
                mi.set_mesh(&mesh);
                self.holder.add_child(&mi);
                self.instances[ci] = Some(mi);
            }
        }
        bytes
    }

    pub fn live_instances(&self) -> usize {
        self.instances.iter().filter(|i| i.is_some()).count()
    }
}
