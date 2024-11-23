use std::{cmp::max, error::Error};

use clap::{
    error::{ContextKind, ContextValue},
    ArgAction, Parser,
};
use image::{Rgb, RgbImage};
use rand::{distributions::WeightedIndex, prelude::Distribution, random, thread_rng, Rng};

/// consumes two from the iterator and makes it a u8 maybe
fn consume_iter_for_u8(iter: &mut impl Iterator<Item = char>) -> u8 {
    (iter.next().unwrap().to_digit(16).unwrap() * 16 + iter.next().unwrap().to_digit(16).unwrap())
        .try_into()
        .unwrap()
}

/// parses `#ff0000` into Rgb(255, 0, 0)
fn parse_hex_optional_octothorpe_to_rgb(input: &str) -> Result<Rgb<u8>, clap::Error> {
    let mut iter = input.trim().trim_start_matches('#').chars();
    if iter.clone().count() != 6 {
        let mut err = clap::Error::new(clap::error::ErrorKind::InvalidValue);
        err.insert(
            ContextKind::InvalidValue,
            ContextValue::String(input.to_owned()),
        );
        return Err(err);
    }
    let r: u8 = consume_iter_for_u8(&mut iter);
    let g: u8 = consume_iter_for_u8(&mut iter);
    let b: u8 = consume_iter_for_u8(&mut iter);

    Ok(Rgb([r, g, b]))
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// image width
    #[arg(long, default_value_t = 4096)]
    width: u32,

    /// image height
    #[arg(long, default_value_t = 2160)]
    height: u32,

    /// split iterations (max 2^n this many squares)
    #[arg(long, default_value_t = 5)]
    levels: usize,

    /// colors to use
    #[arg(long, action=ArgAction::Append, num_args=4, value_parser=parse_hex_optional_octothorpe_to_rgb, default_value = "#ffffff,#ff0000,#ffff00,#0000ff", value_delimiter=',')]
    palette: Vec<Rgb<u8>>,

    // TODO: forward weights
}

trait SplittableGraphic
where
    Self: std::marker::Sized,
{
    fn new(x: u32, y: u32, width: u32, height: u32) -> Self;
    fn split(&self) -> (Self, Self);
}

/// if you have children, you shouldn't have your own item!
#[derive(Debug)]
struct Tree<P>
where
    P: SplittableGraphic + Clone,
{
    item: P,
    left: Option<Box<Tree<P>>>,
    right: Option<Box<Tree<P>>>,
    depth: usize,
}

impl<P: SplittableGraphic + Clone> Tree<P> {
    fn leaves(&self) -> impl Iterator<Item = P> {
        let mut returnable: Vec<P> = vec![];
        if self.left.is_some() || self.right.is_some() {
            if let Some(left) = &self.left {
                returnable.extend(left.leaves());
            }
            if let Some(right) = &self.right {
                returnable.extend(right.leaves());
            }
        } else {
            // TODO: figure out if this cost is acceptable
            returnable.push(self.item.clone());
        }

        returnable.into_iter()
    }
}

impl<P> Tree<P>
where
    P: SplittableGraphic + Clone,
{
    fn new(item: P) -> Self {
        Self {
            item,
            left: None,
            right: None,
            depth: 0,
        }
    }

    /// if max_depth is not fulfilled, call P's split until it is
    fn split(&mut self, max_depth: usize) {
        if self.depth >= max_depth {
            return;
        }

        let (left, right) = self.item.split();
        let mut left_tree = Tree::new(left);
        left_tree.depth = self.depth + 1;
        left_tree.split(max_depth);
        self.left = Some(Box::new(left_tree));

        let mut right_tree = Tree::new(right);
        right_tree.depth = self.depth + 1;
        right_tree.split(max_depth);
        self.right = Some(Box::new(right_tree));
    }
}

#[derive(Debug, Clone)]
struct Rectangle {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl SplittableGraphic for Rectangle {
    fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn split(&self) -> (Self, Self) {
        let width: u32;
        let height;
        let left: Rectangle;
        let right: Rectangle;

        let horz_split: bool;

        // if ratio is fucked, don't randomly select split direction
        if self.width / self.height > 2 {
            horz_split = true
        } else if self.height / self.width > 2 {
            horz_split = false
        } else {
            horz_split = random()
        }

        // TODO: does instantiating this N times cause unnecessary overhead?
        let mut rng = thread_rng();

        if horz_split {
            width = (self.width as f32 * rng.gen_range(0.4..=0.6)).trunc() as u32;
            height = self.height;
            left = Self::new(self.x, self.y, width, height);
            right = Self::new(self.x + width, self.y, self.width - width, height);
        } else {
            width = self.width;
            height = (self.height as f32 * rng.gen_range(0.4..=0.6)).trunc() as u32;
            left = Self::new(self.x, self.y, width, height);
            right = Self::new(self.x, self.y + height, width, self.height - height);
        }
        (left, right)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let mut imagebuf = RgbImage::new(args.width, args.height);

    let root_rectangle = Rectangle::new(0, 0, args.width, args.height);
    let mut tree: Tree<Rectangle> = Tree::new(root_rectangle);
    tree.split(args.levels);

    let leaves = tree.leaves().collect::<Vec<Rectangle>>();

    let mut rng = thread_rng();
    let palette = args.palette;
    let weights: [u8; 4] = [10, 2, 1, 1];
    let dist = WeightedIndex::new(weights).unwrap();

    let border_width: u32 = max(args.width, args.height).div_euclid(1000);

    // assume 0, 0 is top right corner and our rectangle is (0, 0, 3, 3); then to achieve
    // B B B
    // B C B
    // B B B

    for rectangle in leaves {
        let color = palette[dist.sample(&mut rng)];

        // C should be x+B .. x+width-B
        for x in rectangle.x + border_width..rectangle.x.saturating_add(rectangle.width).saturating_sub(border_width) {
            for y in rectangle.y + border_width..rectangle.y.saturating_add(rectangle.height).saturating_sub(border_width) {
                let pixel = imagebuf.get_pixel_mut(x, y);
                *pixel = color;
            }
        }

        // borders

        // top and bottom
        for x in rectangle.x .. rectangle.width {
            for y in (rectangle.y) .. (rectangle.y + border_width) {
                let pixel = imagebuf.get_pixel_mut(x, y);
                *pixel = Rgb([0, 0, 0]);
            }
            for y in (rectangle.y + rectangle.height).saturating_sub(border_width) .. rectangle.y + rectangle.height {
                let pixel = imagebuf.get_pixel_mut(x, y);
                *pixel = Rgb([0, 0, 0]);
            }
        }

        // left and right
        for x in rectangle.x .. rectangle.x + border_width {
            for y in rectangle.y .. rectangle.y + rectangle.height {
                let pixel = imagebuf.get_pixel_mut(x, y);
                *pixel = Rgb([0, 0, 0]);
            }

            for y in rectangle.y + rectangle.height - border_width .. rectangle.y + rectangle.height {
                let pixel = imagebuf.get_pixel_mut(x, y);
                *pixel = Rgb([0, 0, 0]);
            }
        }
    }

    Ok(imagebuf.save("mondrian.png")?)
}
