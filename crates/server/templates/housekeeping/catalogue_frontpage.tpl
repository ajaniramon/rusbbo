{% set title = locale.hk_portada_del_catalogo %}
{% include "housekeeping/base/header.tpl" %}
{% autoescape 'html' %}
<div class="grid g-side">
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_lo_primero_que_se_ve_al_abrir_el_catalogo|raw }}</h2></div>
    <div class="card-body">
      <form class="form" method="post" action="{{ hk.path }}/catalogue/edit_frontpage">
        <input type="hidden" name="hk_token" value="{{ hk.token }}">
        <div class="field"><label>{{ locale.hk_titular|raw }}</label><input type="text" name="header" required maxlength="100" value="{{ frontpageText2 }}"></div>
        <div class="field"><label>{{ locale.hk_texto|raw }}</label><textarea name="subtext" rows="3" required>{{ frontpageText3 }}</textarea></div>
        <div class="field"><label>{{ locale.hk_enlace_web_opcional|raw }}</label><input type="text" name="link" value="{{ frontpageText4 }}" placeholder="{{ locale.hk_http|raw }}"></div>
        <div class="field">
          <label>{{ locale.hk_imagen|raw }}</label>
          <select name="image" id="fp-image">
            {% for image in images %}<option value="{{ image }}" {% if image == frontpageText1 %}selected{% endif %}>{{ image }}</option>{% endfor %}
          </select>
          {% if images is empty %}<span class="help">{{ locale.hk_no_se_han_encontrado_imagenes_en_c_images_top_st|raw }}</span>{% endif %}
        </div>
        <div class="form-actions"><button class="btn primary" type="submit">{{ locale.hk_guardar_portada|raw }}</button><span class="help">{{ locale.hk_se_envia_al_juego_al_momento|raw }}</span></div>
      </form>
    </div>
  </div>
  <div class="card">
    <div class="card-head"><h2>{{ locale.hk_vista_previa|raw }}</h2></div>
    <div class="card-body">
      <div class="preview-box"><img id="fp-preview" src="{{ site.staticContentPath }}/c_images/Top_Story_Images/{{ frontpageText1 }}" alt="" onerror="this.style.visibility='hidden'" onload="this.style.visibility='visible'"></div>
      <p class="muted small">{{ locale.hk_imagen_actual|raw }} <span class="mono">{{ frontpageText1 }}</span></p>
    </div>
  </div>
</div>
<script>
(function () {
  var select = document.getElementById('fp-image');
  var img = document.getElementById('fp-preview');
  if (!select || !img) return;
  var base = '{{ site.staticContentPath | escape('js') }}/c_images/Top_Story_Images/';
  function update() { img.src = base + encodeURIComponent(select.value); }
  select.addEventListener('change', update);
  select.addEventListener('keyup', update);
})();
</script>
{% endautoescape %}
{% include "housekeeping/base/footer.tpl" %}
