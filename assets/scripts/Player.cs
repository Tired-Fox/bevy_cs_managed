using System;
using Engine;

public class Player {
    public Vector3 Position;

    void Awake() {
        Console.WriteLine($"[C#] Awake: <{Position.x}, {Position.y}, {Position.z}>");
    }

    void Update(float dt) {
        Console.WriteLine($"[C#] Delta Time: {dt}");
        Position.x += dt;
        Position.y += dt;
        Position.z += dt;
        Console.WriteLine($"[C#] Update: <{Position.x}, {Position.y}, {Position.z}>");
    }
}
