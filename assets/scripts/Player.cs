using System;
using Engine;

public class Player {
    public Vector3 Position { get; set; }

    void Awake() {
        Console.WriteLine($"[C#] Awake: {Position}");
    }

    void Update(float dt) {
        Position = (Position + dt) % 5;
        Console.WriteLine($"[C#] Update: pos: {Position}");
    }
}
