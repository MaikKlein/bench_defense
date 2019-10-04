extern crate env_logger;
extern crate ggez;
extern crate itertools;
extern crate pyro;
extern crate rand;
use ggez::nalgebra as na;
use ggez::*;
use itertools::Itertools;
use pyro::*;
use rand::{thread_rng, Rng};
use std::env;
use std::f32::consts::PI;
use std::path;
use std::time::{Duration, Instant, SystemTime};

#[derive(Copy, Clone)]
pub struct Position(pub na::Point2<f32>);
#[derive(Copy, Clone)]
pub struct Velocity(pub na::Vector2<f32>);
#[derive(Copy, Clone)]
pub struct Speed(pub f32);
#[derive(Copy, Clone)]
pub struct Enemy {
    pub health: f32,
}

#[derive(Copy, Clone)]
pub struct Explosion {
    pub radius: f32,
    pub max_radius: f32,
}

#[derive(Copy, Clone)]
pub struct Damage(pub f32);

pub struct TimeToLive {
    pub created: SystemTime,
    pub time_until_death: Duration,
}
pub type Missile<Projectile> = (
    Position,
    Velocity,
    Render,
    Orientation,
    TimeToLive,
    Flip,
    Damage,
    Projectile,
);
pub struct Bullet;
pub type BulletEntity = (
    Position,
    Velocity,
    Render,
    Orientation,
    TimeToLive,
    Flip,
    Bullet,
);

pub struct Render {
    pub asset: AssetId,
    pub scale: f32,
    pub inital_rotation: f32,
}

pub struct MoveTorwards {
    pub destination: na::Point2<f32>,
    pub side: usize,
}
pub struct Orientation(pub f32);
#[derive(Copy, Clone)]
pub enum Flip {
    Left,
    Right,
}

#[derive(Copy, Clone)]
pub struct Shoot {
    pub recover: Recover,
}

#[derive(Copy, Clone)]
pub struct DeltaTime(pub f32);

#[derive(Copy, Clone)]
pub struct Recover {
    pub last_action: Option<Instant>,
    pub recover: Duration,
}
impl Recover {
    pub fn new(recover: Duration) -> Self {
        Self {
            last_action: None,
            recover,
        }
    }
    pub fn action(&mut self) -> Option<()> {
        if let Some(last_action) = self.last_action {
            let duration = Instant::now().duration_since(last_action);
            if duration < self.recover {
                None
            } else {
                self.last_action = Some(Instant::now());
                Some(())
            }
        } else {
            self.last_action = Some(Instant::now());
            Some(())
        }
    }
}

pub fn shoot_at_enemy(world: &mut World) {
    let projectiles: Vec<_> = world
        .matcher::<All<(Read<Position>, Write<Shoot>)>>()
        .filter_map(|(&spawn_pos, shoot)| shoot.recover.action().map(move |_| spawn_pos))
        .flat_map(|spawn_pos| {
            world
                .matcher::<All<(Read<Position>, Read<Enemy>)>>()
                .take(10)
                .map(move |(&target_pos, _)| {
                    let dir = (target_pos.0 - spawn_pos.0).normalize();
                    let offset = dir * 30.0;
                    let new_pos = Position(spawn_pos.0 + offset);
                    create_missile(AssetId::Missile, new_pos, dir, 700.0, SpawnMissile {})
                })
        }).collect();
    world.append_components(projectiles);
}

pub fn update_orientation(world: &mut World) {
    world
        .matcher::<All<(Read<Velocity>, Write<Orientation>)>>()
        .for_each(|(vel, orientation)| {
            let dir = vel.0.normalize();
            let mut angle = dir.angle(&na::Vector2::new(1.0, 0.0));
            if dir.y < 0.0 {
                angle = -angle;
            }
            orientation.0 = angle;
        });
}

pub fn move_torwards(world: &mut World, dt: DeltaTime) {
    world
        .matcher::<All<(
            Write<Position>,
            Read<MoveTorwards>,
            Read<Speed>,
            Write<Flip>,
        )>>().for_each(|(pos, target, speed, flip)| {
            let dir = (target.destination - pos.0).normalize();
            pos.0 += dir * speed.0 * dt.0;
            let up = na::Vector2::new(1.0, 0.0);
            let angle = up.angle(&dir);

            *flip = if angle > PI / 2.0 {
                Flip::Left
            } else {
                Flip::Right
            };
        });
}

pub fn update_destination(world: &mut World, sides: &Sides) {
    world
        .matcher::<All<(Read<Position>, Write<MoveTorwards>)>>()
        .for_each(|(pos, target)| {
            let distance = na::distance(&target.destination, &pos.0);
            if distance <= 1.0 {
                *target = sides.get_random_point(target.side);
            }
        });
}

pub fn create_radial_missiles<Projectile: Component + Copy>(
    pos: Position,
    speed: f32,
    offset: f32,
    count: usize,
    projectile: Projectile,
) -> impl Iterator<Item = Missile<Projectile>> {
    let step_size = 2.0 * PI / count as f32;
    (0..count)
        .scan(0.0, move |acc, _| {
            *acc += step_size;
            Some(*acc)
        }).map(move |angle| {
            let x = offset * f32::cos(angle);
            let y = offset * f32::sin(angle);
            let dir = na::Vector2::new(x, y).normalize();
            create_missile(AssetId::SmallMissile, pos, dir, speed, projectile)
        })
}

pub fn kill_enemies(world: &mut World) {
    let dead_enemies: Vec<_> = world
        .matcher_with_entities::<All<(Read<Enemy>,)>>()
        .filter_map(|(entity, (enemy,))| {
            if enemy.health <= 0.0 {
                Some(entity)
            } else {
                None
            }
        }).collect();
    world.remove_entities(dead_enemies);
}

pub trait OnProjectileHit {
    type Projectile: Component + Sized;
    fn finish(&mut self, _world: &mut World) {}
    fn on_projectile_hit(&mut self, pos: Position, projectile: &Self::Projectile);
    fn hit(&mut self, world: &mut World) {
        const HIT_RADIUS: f32 = 10.0;
        let mut explosions = Vec::new();
        let mut entities = Vec::new();
        world
            .matcher_with_entities::<All<(Read<Self::Projectile>, Read<Position>, Read<Damage>)>>()
            .for_each(|(entity, (projectile, &missile, damage))| {
                let colliding_enemy = world
                    .matcher::<All<(Write<Enemy>, Read<Position>)>>()
                    .find_map(|(enemy, enemy_pos)| {
                        if na::distance(&missile.0, &enemy_pos.0) <= HIT_RADIUS {
                            Some(enemy)
                        } else {
                            None
                        }
                    });

                if let Some(enemy) = colliding_enemy {
                    enemy.health -= damage.0;
                    self.on_projectile_hit(missile, projectile);
                    explosions.push((
                        Explosion {
                            radius: 0.0,
                            max_radius: 25.0,
                        },
                        missile,
                    ));
                    entities.push(entity);
                }
            });
        world.append_components(explosions);
        world.remove_entities(entities);
        self.finish(world);
    }
}

#[derive(Copy, Clone)]
pub struct StandardMissile;
pub struct StandardMissileSystem;
impl StandardMissileSystem {
    pub fn new() -> Self {
        StandardMissileSystem {}
    }
}
impl OnProjectileHit for StandardMissileSystem {
    type Projectile = StandardMissile;
    fn on_projectile_hit(&mut self, _pos: Position, _projectile: &Self::Projectile) {}
}

#[derive(Copy, Clone)]
pub struct SpawnMissile;
pub struct SpawnMissileSystem {
    spawn: Vec<Missile<StandardMissile>>,
}
impl SpawnMissileSystem {
    pub fn new() -> Self {
        Self { spawn: Vec::new() }
    }
}
impl OnProjectileHit for SpawnMissileSystem {
    type Projectile = SpawnMissile;
    fn on_projectile_hit(&mut self, pos: Position, _projectile: &Self::Projectile) {
        let missiles = create_radial_missiles(pos, 150.0, 15.0, 12, StandardMissile {});
        self.spawn.extend(missiles);
    }
    fn finish(&mut self, world: &mut World) {
        let spawn = self.spawn.drain(0..);
        world.append_components(spawn);
    }
}

pub fn animate_explosion(world: &mut World, dt: DeltaTime) {
    const EXPANSION_SPEED: f32 = 25.0;
    world
        .matcher::<All<(Write<Explosion>, Read<Position>)>>()
        .for_each(|(explosion, _)| {
            explosion.radius += EXPANSION_SPEED * dt.0;
        });
    let entities: Vec<_> = world
        .matcher_with_entities::<All<(Write<Explosion>,)>>()
        .filter_map(|(entity, (explosion,))| {
            if explosion.radius >= explosion.max_radius {
                Some(entity)
            } else {
                None
            }
        }).collect();
    world.remove_entities(entities);
}
pub fn draw_explosion(store: &AssetStore, ctx: &mut Context, world: &mut World) -> GameResult {
    let circle = &store.assets[AssetId::Explosion as usize];
    let mut batch = graphics::spritebatch::SpriteBatch::new(circle.image.clone());
    world
        .matcher::<All<(Read<Explosion>, Read<Position>)>>()
        .for_each(|(explosion, pos)| {
            let alpha = 1.0 - explosion.radius * 255.0 / explosion.max_radius;
            let param = graphics::DrawParam::new()
                .dest(na::Point2::new(pos.0.x, pos.0.y))
                .rotation(0.0)
                .offset(na::Point2::new(0.5, 0.5))
                .scale(na::Vector2::new(circle.scale, circle.scale) * explosion.radius)
                .color(graphics::Color::from_rgba(255, 0, 0, alpha as u8));
            batch.add(param);
        });
    graphics::draw(ctx, &batch, graphics::DrawParam::default())
}
pub fn draw(store: &AssetStore, world: &mut World, ctx: &mut Context) -> GameResult {
    let submisson = world
        .matcher::<All<(Read<Position>, Read<Orientation>, Read<Flip>, Read<Render>)>>()
        .sorted_by(|(_, _, _, left), (_, _, _, right)| Ord::cmp(&left.asset, &right.asset))
        .into_iter()
        .group_by(|(_, _, _, render)| render.asset);

    for (key, group) in &submisson {
        let asset = &store.assets[key as usize];
        let image = asset.image.clone();
        let mut batch = graphics::spritebatch::SpriteBatch::new(image);
        for (pos, orientation, flip, render) in group {
            let scale_y = match flip {
                Flip::Left => 1.0,
                Flip::Right => -1.0,
            };
            let param = graphics::DrawParam::new()
                .dest(na::Point2::new(pos.0.x, pos.0.y))
                .rotation(orientation.0 + asset.rotation)
                .offset(na::Point2::new(0.5, 0.5))
                .scale(na::Vector2::new(render.scale * scale_y, render.scale) * asset.scale);
            batch.add(param);
        }
        graphics::draw(ctx, &batch, graphics::DrawParam::default())?;
    }
    Ok(())
}
pub fn create_bullet(location: Position, target: Position, speed: f32) -> BulletEntity {
    let dir = (target.0 - location.0).normalize() * speed;
    (
        location,
        Velocity(dir),
        Render {
            asset: AssetId::Missile,
            scale: 0.2,
            inital_rotation: PI / 2.0,
        },
        Orientation(0.0),
        TimeToLive {
            created: SystemTime::now(),
            time_until_death: Duration::from_secs(3),
        },
        Flip::Right,
        Bullet {},
    )
}
pub fn create_missile<Projectile: Component>(
    asset: AssetId,
    location: Position,
    dir: na::Vector2<f32>,
    speed: f32,
    projectile: Projectile,
) -> Missile<Projectile> {
    (
        location,
        Velocity(dir * speed),
        Render {
            asset,
            scale: 1.0,
            inital_rotation: PI / 2.0,
        },
        Orientation(0.0),
        TimeToLive {
            created: SystemTime::now(),
            time_until_death: Duration::from_secs(3),
        },
        Flip::Right,
        Damage(1.0),
        projectile,
    )
}

pub fn kill_entities(world: &mut World) {
    let entities: Vec<_> = world
        .matcher_with_entities::<All<(Read<TimeToLive>,)>>()
        .filter_map(|(entity, (time,))| {
            let now = SystemTime::now();
            if now.duration_since(time.created).unwrap() >= time.time_until_death {
                Some(entity)
            } else {
                None
            }
        }).collect();
    world.remove_entities(entities);
}

pub fn move_velocity(world: &mut World, dt: DeltaTime) {
    world
        .matcher::<All<(Write<Position>, Read<Velocity>)>>()
        .for_each(|(pos, vel)| {
            pos.0 += vel.0 * dt.0;
        })
}

pub fn spawn_random_grunts(world: &mut World, count: usize, sides: &Sides) {
    let ships = (0..count).map(|_| {
        let move_torwards = sides.get_random_point(sides.get_random_side());
        (
            Position(sides.get_random_point(move_torwards.side).destination),
            move_torwards,
            Orientation(0.0),
            Speed(thread_rng().gen_range(150.0, 200.0)),
            Render {
                asset: AssetId::Grunt,
                scale: 1.0,
                inital_rotation: 0.0,
            },
            Flip::Right,
            Enemy { health: 100.0 },
        )
    });
    world.append_components(ships);
}

// Let's not overcomplicate the asset loading system for a simple demo
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum AssetId {
    Grunt = 0,
    Missile = 1,
    SmallMissile = 2,
    Tower = 3,
    Explosion = 4,
}

pub struct AssetSettings {
    pub scale: f32,
    pub rotation: f32,
    pub image: graphics::Image,
}

pub struct AssetStore {
    assets: Vec<AssetSettings>,
}

impl AssetStore {
    pub fn load(ctx: &mut Context) -> GameResult<AssetStore> {
        let assets = vec![
            AssetSettings {
                scale: 1.0,
                rotation: 0.0,
                image: graphics::Image::new(ctx, "/grunt.png")?,
            },
            AssetSettings {
                scale: 1.0,
                rotation: PI / 2.0,
                image: graphics::Image::new(ctx, "/missile1.png")?,
            },
            AssetSettings {
                scale: 0.7,
                rotation: PI / 2.0,
                image: graphics::Image::new(ctx, "/missile2.png")?,
            },
            AssetSettings {
                scale: 1.0,
                rotation: 0.0,
                image: graphics::Image::new(ctx, "/tower.png")?,
            },
            AssetSettings {
                scale: 0.01,
                rotation: 0.0,
                image: graphics::Image::new(ctx, "/explosion.png")?,
            },
        ];
        Ok(AssetStore { assets })
    }
}

pub struct Sides {
    waypoints: [Waypoints; 2],
}
impl Sides {
    pub fn new((width, height): (f32, f32), spacing: f32, count: usize) -> Sides {
        let left = Waypoints::line((spacing, height), spacing, count);
        let right = Waypoints::line((width - spacing, height), spacing, count);
        Sides {
            waypoints: [left, right],
        }
    }

    pub fn get_random_side(&self) -> usize {
        thread_rng().gen_range(0, self.waypoints.len())
    }
    pub fn get_random_point(&self, previous_side: usize) -> MoveTorwards {
        let next_side = (previous_side + 1) % self.waypoints.len();
        MoveTorwards {
            destination: self.waypoints[next_side].get_random_point(),
            side: next_side,
        }
    }
}

pub struct Waypoints {
    pub points: Vec<na::Point2<f32>>,
}

impl Waypoints {
    pub fn line((offset, height): (f32, f32), spacing: f32, count: usize) -> Self {
        let adjusted_height = height - spacing;
        let step = (adjusted_height - spacing) / count as f32;
        let create_waypoints = |offset: f32| {
            (0..count).scan(na::Point2::new(0.0f32, spacing), move |state, _| {
                *state += na::Vector2::new(0.0, step);
                state.x = offset;
                Some(*state)
            })
        };
        let mut points = Vec::new();
        points.extend(create_waypoints(offset));
        Waypoints { points }
    }
    pub fn get_random_point(&self) -> na::Point2<f32> {
        let index: usize = thread_rng().gen_range(0, self.points.len());
        self.points[index]
    }
}
pub struct EnemySpawner {
    pub enemies_to_spawn: usize,
}
impl EnemySpawner {
    pub fn spawn_enemies(&mut self, world: &mut World, sides: &Sides) {
        let living_enemies = world.matcher::<All<(Read<Enemy>,)>>().count();
        if living_enemies > 0 {
            return;
        }
        spawn_random_grunts(world, self.enemies_to_spawn, &sides);
    }
}

struct MainState {
    world: World,
    sides: Sides,
    store: AssetStore,
    font: graphics::Font,
    spawner: EnemySpawner,
}

pub fn spawn_towers(world: &mut World, (width, height): (f32, f32), offset: f32) {
    let spawn_points = [
        na::Point2::new(0.0 + offset, 0.0 + offset),
        na::Point2::new(width - offset, 0.0 + offset),
        na::Point2::new(width - offset, height - offset),
        na::Point2::new(0.0 + offset, height - offset),
    ];
    let towers = spawn_points.iter().map(|&pos| {
        (
            Position(pos),
            Render {
                asset: AssetId::Tower,
                scale: 1.0,
                inital_rotation: 0.0,
            },
            Shoot {
                recover: Recover::new(Duration::from_millis(250)),
            },
            Orientation(0.0),
            Flip::Right,
        )
    });
    world.append_components(towers);
}
impl MainState {
    fn new(ctx: &mut Context) -> GameResult<MainState> {
        let mut world = World::new();
        let screen_coords = graphics::screen_coordinates(ctx);
        let size = (
            screen_coords.w,
            screen_coords.h,
        );
        let store = AssetStore::load(ctx).expect("Unable to load assets");
        let sides = Sides::new(size, 100.0, 100);
        let font = graphics::Font::new(ctx, "/DejaVuSerif.ttf")?;
        spawn_towers(&mut world, size, 50.0);
        let spawner = EnemySpawner {
            enemies_to_spawn: 500,
        };
        let s = MainState {
            world,
            store,
            sides,
            font,
            spawner,
        };
        Ok(s)
    }
}
impl event::EventHandler for MainState {
    fn update(&mut self, ctx: &mut Context) -> GameResult<()> {
        let dt = DeltaTime(timer::duration_to_f64(timer::delta(ctx)) as f32);
        let world = &mut self.world;
        self.spawner.spawn_enemies(world, &self.sides);
        move_torwards(world, dt);
        update_destination(world, &self.sides);
        move_velocity(world, dt);
        kill_entities(world);
        update_orientation(world);
        animate_explosion(world, dt);
        shoot_at_enemy(world);
        kill_enemies(world);
        StandardMissileSystem::new().hit(world);
        SpawnMissileSystem::new().hit(world);
        Ok(())
    }

    fn draw(&mut self, ctx: &mut Context) -> GameResult {
        let fps = timer::fps(ctx) as u64;
        graphics::clear(ctx, graphics::Color::from_rgb(40, 220, 70));
        draw(&self.store, &mut self.world, ctx)?;
        draw_explosion(&self.store, ctx, &mut self.world)?;
        let count = self.world.matcher::<All<(Read<Enemy>,)>>().count();
        let tf = graphics::TextFragment {
            text: format!("FPS: {}, Enemies: {}", fps, count),
            font: Some(self.font),
            scale: Some(graphics::Scale::uniform(18.0)),
            ..Default::default()
        };
        let text = graphics::Text::new(tf);
        let text_param = graphics::DrawParam::new()
            .dest(na::Point2::new(0.0, 0.0));
        graphics::draw(ctx, &text, text_param)?;
        graphics::present(ctx)
    }
}

pub fn main() -> GameResult {
    env_logger::init();
    let resources = env::var("CARGO_MANIFEST_DIR")
        .map(path::PathBuf::from)
        .map(|mut path| {
            path.push("resources");
            path
        })
        .map_err(|err| GameError::FilesystemError(err.to_string()))?;
    let (ctx, event_loop) = {
        &mut ContextBuilder::new("super_simple", "ggez")
            .add_resource_path(resources)
            .build()?
    };
    let state = &mut MainState::new(ctx)?;
    event::run(ctx, event_loop, state)
}
