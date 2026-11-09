{% set title = locale.hk_editar_sala ~ room.name() %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="page-head">
  <div><p>{{ locale.hk_sala_3|raw }}{{ room.id() }} · {% if room.ownerId() > 0 %}{{ locale.hk_de|raw }} {{ room.ownerName() }}{% else %}{{ locale.hk_publica|raw }}{% endif %} · {{ room.visitorsNow() }} {{ locale.hk_dentro_ahora|raw }}</p></div>
  <div class="actions"><a class="btn" href="{{ hk.path }}/rooms/view?id={{ room.id() }}">{{ locale.hk_ver_la_sala|raw }}</a></div>
</div>

<div class="card">
  <div class="card-body">
    <form class="form" method="post" action="{{ hk.path }}/rooms/edit?id={{ room.id() }}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <div class="row r-2">
        <div class="field"><label>{{ locale.hk_nombre|raw }}</label><input type="text" name="name" value="{{ room.name() }}" required></div>
        <div class="field"><label>{{ locale.hk_descripcion|raw }}</label><input type="text" name="description" value="{{ room.description() }}"></div>
      </div>
      <div class="row">
        <div class="field"><label>{{ locale.hk_id_del_dueno|raw }}</label><input type="number" name="owner_id" value="{{ room.ownerId() }}"><span class="help">{{ locale.hk_0_sala_publica|raw }}</span></div>
        <div class="field"><label>{{ locale.hk_categoria|raw }}</label>
          <select name="category">{% for c in categories %}<option value="{{ c.id() }}" {% if room.categoryId() == c.id() %}selected{% endif %}>{{ c.name() }} (#{{ c.id() }})</option>{% endfor %}</select>
        </div>
        <div class="field"><label>{{ locale.hk_modelo|raw }}</label>
          <select name="model">{% for m in models %}<option value="{{ m.modelId() }}" {% if room.model() == m.modelId() %}selected{% endif %}>{{ m.modelId() }}</option>{% endfor %}</select>
        </div>
        <div class="field"><label>{{ locale.hk_acceso|raw }}</label>
          <select name="accesstype">
            <option value="0" {% if room.accessType() == 0 %}selected{% endif %}>{{ locale.hk_abierta|raw }}</option>
            <option value="1" {% if room.accessType() == 1 %}selected{% endif %}>{{ locale.hk_con_timbre|raw }}</option>
            <option value="2" {% if room.accessType() == 2 %}selected{% endif %}>{{ locale.hk_con_contrasena|raw }}</option>
          </select>
        </div>
      </div>
      <div class="row">
        <div class="field"><label>{{ locale.hk_contrasena|raw }}</label><input type="text" name="password" value="{{ room.password() }}"></div>
        <div class="field"><label>{{ locale.hk_aforo_maximo|raw }}</label><input type="number" name="visitors_max" min="1" value="{{ room.visitorsMax() }}"></div>
        <div class="field"><label>{{ locale.hk_puntuacion|raw }}</label><input type="number" name="rating" value="{{ room.rating() }}"></div>
        <div class="field"><label>{{ locale.hk_id_de_grupo|raw }}</label><input type="number" name="group_id" value="{{ room.groupId() }}"></div>
      </div>
      <div class="row">
        <div class="field"><label>{{ locale.hk_papel_pared|raw }}</label><input type="number" name="wallpaper" value="{{ room.wallpaper() }}"></div>
        <div class="field"><label>{{ locale.hk_suelo|raw }}</label><input type="number" name="floor" value="{{ room.floor() }}"></div>
        <div class="field"><label>{{ locale.hk_paisaje|raw }}</label><input type="text" name="landscape" value="{{ room.landscape() }}"></div>
        <div class="field"><label>{{ locale.hk_ccts|raw }}</label><input type="text" name="ccts" value="{{ room.ccts() }}"></div>
        <div class="field"><label>{{ locale.hk_icono|raw }}</label><input type="text" name="icon_data" value="{{ room.iconData() }}"></div>
      </div>
      <div class="form-actions">
        <label class="check"><input type="checkbox" name="showname" {% if room.showOwnerName() %}checked{% endif %}> {{ locale.hk_mostrar_el_nombre_del_dueno|raw }}</label>
        <label class="check"><input type="checkbox" name="superusers" {% if room.superUsers() %}checked{% endif %}> {{ locale.hk_todos_pueden_mover_furnis|raw }}</label>
        <label class="check"><input type="checkbox" name="is_hidden" {% if room.hidden() %}checked{% endif %}> {{ locale.hk_oculta_del_navegador|raw }}</label>
      </div>
      <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_guardar_sala|raw }}</button><span class="help">{{ locale.hk_si_la_sala_esta_cargada_en_el_juego_se_cerrara_y|raw }}</span></div>
    </form>
  </div>
</div>

<div class="grid g-4 mt">
  <div class="card"><div class="card-head"><h2>{{ locale.hk_con_derechos|raw }}</h2></div><div class="card-body">{% for r in rights %}<div>{{ r.username() }} <span class="muted small">#{{ r.userId() }}</span></div>{% else %}<span class="muted">{{ locale.hk_nadie|raw }}</span>{% endfor %}</div></div>
  <div class="card"><div class="card-head"><h2>{{ locale.hk_baneados|raw }}</h2></div><div class="card-body">{% for b in bans %}<div>{{ b.username() }} <span class="muted small">#{{ b.userId() }}</span></div>{% else %}<span class="muted">{{ locale.hk_nadie|raw }}</span>{% endfor %}</div></div>
  <div class="card"><div class="card-head"><h2>{{ locale.hk_eventos|raw }}</h2></div><div class="card-body">{% for e in events %}<div><strong>{{ e.name() }}</strong> <span class="muted small">{{ locale.hk_por|raw }} {{ e.username() }}</span></div>{% else %}<span class="muted">{{ locale.hk_ninguno|raw }}</span>{% endfor %}</div></div>
  <div class="card"><div class="card-head"><h2>{{ locale.hk_placas_de_entrada|raw }}</h2></div><div class="card-body">{% for b in entryBadges %}<span class="pill">{{ b }}</span> {% else %}<span class="muted">{{ locale.hk_ninguna|raw }}</span>{% endfor %}</div></div>
</div>

<div class="card danger-zone mt">
  <div class="card-head"><h2>{{ locale.hk_zona_peligrosa|raw }}</h2></div>
  <div class="card-body form-actions">
    <form class="inline" method="post" action="{{ hk.path }}/rooms/delete?id={{ room.id() }}" data-confirm="{{ locale.hk_borrar_la_sala|raw }}{{ room.name() }}{{ locale.hk_para_siempre_no_se_puede_deshacer|raw }}">
      <input type="hidden" name="hk_token" value="{{ hk.token }}">
      <button class="btn danger" type="submit">{{ locale.hk_borrar_sala|raw }}</button>
    </form>
    <span class="help">{{ locale.hk_borra_la_sala_de_la_base_de_datos_usalo_solo_con|raw }}</span>
  </div>
</div>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
