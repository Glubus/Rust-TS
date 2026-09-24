//! Tuple arities supported by the schema and codec implementations.

/// Invokes `$callback!(len => index Type, ...)` once for every tuple arity from 1 to 12.
macro_rules! for_each_tuple {
    ($callback:ident) => {
        $callback!(1 => 0 A);
        $callback!(2 => 0 A, 1 B);
        $callback!(3 => 0 A, 1 B, 2 C);
        $callback!(4 => 0 A, 1 B, 2 C, 3 D);
        $callback!(5 => 0 A, 1 B, 2 C, 3 D, 4 E);
        $callback!(6 => 0 A, 1 B, 2 C, 3 D, 4 E, 5 F);
        $callback!(7 => 0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G);
        $callback!(8 => 0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H);
        $callback!(9 => 0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H, 8 I);
        $callback!(10 => 0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H, 8 I, 9 J);
        $callback!(11 => 0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H, 8 I, 9 J, 10 K);
        $callback!(12 => 0 A, 1 B, 2 C, 3 D, 4 E, 5 F, 6 G, 7 H, 8 I, 9 J, 10 K, 11 L);
    };
}

pub(crate) use for_each_tuple;
